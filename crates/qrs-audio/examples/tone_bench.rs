//! Tone cost (release build) and aliasing: a 2.7 kHz sine through each preset; energy at frequencies that are
//! not harmonics of 2.7 kHz (aliases folded back from above Nyquist) relative to the total.
use qrs_audio::tone::{Preset, ToneSpec, make_tone};

fn dft_power(x: &[f32], rate: f32, f: f32) -> f32 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, s) in x.iter().enumerate() {
        let w = std::f64::consts::TAU * f as f64 * i as f64 / rate as f64;
        re += *s as f64 * w.cos();
        im -= *s as f64 * w.sin();
    }
    ((re * re + im * im) / (x.len() as f64).powi(2)) as f32
}

fn main() {
    let rate = 48000.0;
    for p in [
        Preset::Clean,
        Preset::Crunch,
        Preset::Lead,
        Preset::Bass,
        Preset::Acoustic,
    ] {
        let mut t = make_tone(&ToneSpec::Preset { preset: p }, 48000).unwrap();
        let mut x: Vec<f32> = (0..48000 * 5)
            .map(|i| (std::f32::consts::TAU * 2700.0 * i as f32 / rate).sin() * 0.3)
            .collect();
        let start = std::time::Instant::now();
        for c in x.chunks_mut(128) {
            t.process(c);
        }
        let el = start.elapsed().as_secs_f64();
        let tail = &x[48000 * 4..];
        let harm: f32 = (1..9).map(|k| dft_power(tail, rate, 2700.0 * k as f32)).sum();
        // aliases of the 9th..20th harmonics land at |k*2700 - n*48000|
        let mut alias = 0.0;
        for k in 9..20 {
            let mut f = (k as f32 * 2700.0) % 48000.0;
            if f > 24000.0 {
                f = 48000.0 - f;
            }
            if (f / 2700.0 - (f / 2700.0).round()).abs() > 0.05 {
                alias += dft_power(tail, rate, f);
            }
        }
        println!(
            "{:<9} {:>6.2}% CPU of one core   aliases {:>6.1} dB below the harmonics",
            p.label(),
            el / 5.0 * 100.0,
            10.0 * (harm / alias.max(1e-20)).log10()
        );
    }
}
