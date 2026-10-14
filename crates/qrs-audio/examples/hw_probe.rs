//! Hardware check, SILENT: lists devices, then runs the engine on an input and an output for a few seconds with
//! monitoring off and no song, and reports what the streams did.
//!   cargo run -p qrs-audio --example hw_probe -- [input-name-part] [output-name-part] [seconds] [exclusive]
use std::time::Duration;

use qrs_audio::backend::{self, Backend, BufferSize};
use qrs_audio::{Engine, InputRoute, Route};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let devices = backend::enumerate();
    for d in &devices {
        println!(
            "{:<7} in {:>2} out {:>2} {:>6} Hz {}{}  {}  usb {:?} bus {:?} adapter {:?}",
            d.id.backend.label(),
            d.input_channels,
            d.output_channels,
            d.default_rate,
            d.label(),
            if d.is_default_input {
                " [default in]"
            } else if d.is_default_output {
                " [default out]"
            } else {
                ""
            },
            d.hardware
                .instance_paths
                .first()
                .map(|s| s.as_str())
                .unwrap_or(""),
            d.hardware.usb.map(|(v, p)| format!("{v:04X}:{p:04X}")),
            d.hardware.bus,
            d.hardware.adapter_name,
        );
    }
    let pick = |part: &str, input: bool| {
        devices.iter().find(|d| {
            d.id.backend != Backend::Null
                && d.label().to_lowercase().contains(&part.to_lowercase())
                && if input {
                    d.input_channels > 0
                } else {
                    d.output_channels > 0
                }
        })
    };
    let Some(inp) = a.get(1).and_then(|p| pick(p, true)) else {
        return;
    };
    let out = a.get(2).and_then(|p| pick(p, false));
    let secs: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);
    let exclusive = a.get(4).map(|s| s == "exclusive").unwrap_or(false);
    let e = Engine::new();
    e.set_monitor(false); // never make a sound
    e.set_song_gain(0.0);
    e.open(Route {
        input: Some(InputRoute {
            device: inp.id.clone(),
            channel: -1,
        }),
        output: out.map(|o| o.id.clone()),
        exclusive,
        buffer: BufferSize::Default,
        base_channel: 0,
    });
    e.settle(Duration::from_secs(3));
    std::thread::sleep(Duration::from_millis(300));
    e.reset_stats();
    let st = e.status();
    println!("\nstatus: {st:#?}");
    let mut max_peak = [0f32; 4];
    for _ in 0..secs * 10 {
        std::thread::sleep(Duration::from_millis(100));
        let m = e.meters();
        for c in 0..4 {
            max_peak[c] = max_peak[c].max(m.channels[c]);
        }
    }
    let s = e.stats();
    let in_rate = st
        .input
        .as_ref()
        .map(|i| i.rate as f64 / i.block.max(1) as f64)
        .unwrap_or(0.0);
    println!(
        "after {secs} s: {s:?}\n  input callbacks/s {:.1} (expected ~{in_rate:.1}), output callbacks/s {:.1}, max work {:.3} ms, channel peaks {max_peak:?}, onsets {}",
        s.in_callbacks as f64 / secs as f64,
        s.out_callbacks as f64 / secs as f64,
        s.max_work_ns as f64 / 1e6,
        e.take_onsets().len()
    );
    for ev in e.events() {
        println!("event {ev:?}");
    }
}
