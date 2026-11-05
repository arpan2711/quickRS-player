//! Synthetic test material (P32.6, new in quickRS): plucked bass notes for the bass mode's tests, made here so
//! every crate's tests can use the same strings (there's no real bass to record). Deterministic (seeded noise).
//!
//! - `ks_pluck`: a Karplus-Strong string: the plucked string's shape (a triangle, with some noise), softened by a low-pass for a finger's attack, round a
//!   delay line one period long with an averaging filter in the loop (the highs die first, as a real string's).
//! - `additive_pluck`: chosen partials with a ramped attack and an exponential decay, for a timbre the string
//!   model doesn't make (a weak fundamental under a strong 2nd harmonic, as many bass pickups give).

use crate::pitch::midi_to_hz;

/// A seeded noise source (-1..1).
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Noise {
        Noise(seed.wrapping_mul(2654435761).wrapping_add(12345))
    }

    pub fn sample(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

/// A plucked string at `midi` (fractional allowed), `secs` long, its peak `amp`. `soft` 0..0.95 low-passes the
/// excitation (0: a pick's bright attack, 0.6 and up: a finger's). It rings with a T60 of about 4 s low down.
pub fn ks_pluck(midi: f64, rate: u32, secs: f64, amp: f32, soft: f32, seed: u32) -> Vec<f32> {
    let f = midi_to_hz(midi);
    let len = (secs * rate as f64) as usize;
    // the loop's delay is the period less the averaging filter's half sample; a linear interpolation for the
    // fraction keeps it in tune within a cent
    let d = (rate as f64 / f - 0.5).max(2.0);
    let n = d.ceil() as usize + 2;
    let mut noise = Noise::new(seed);
    let mut out = vec![0.0f32; len.max(n)];
    let a = soft.clamp(0.0, 0.95);
    // the string's shape when let go: a triangle with its apex a fifth of the way along (a finger near the
    // neck pickup), a little noise on it (a real pluck isn't clean)
    let mut burst: Vec<f32> = (0..n)
        .map(|i| {
            let u = (i as f64 / d).fract();
            let tri = if u < 0.2 { u / 0.2 } else { (1.0 - u) / 0.8 };
            tri as f32 + 0.1 * noise.sample()
        })
        .collect();
    for _ in 0..2 {
        let mut y = 0.0f32;
        for v in burst.iter_mut() {
            y = (1.0 - a) * *v + a * y;
            *v = y;
        }
    }
    let mean = burst.iter().sum::<f32>() / n as f32;
    for (o, v) in out.iter_mut().zip(&burst) {
        *o = v - mean;
    }
    // loss per trip round the loop: 60 dB down in t60 seconds
    let t60 = 4.0f64.min(2.0 + 200.0 / f);
    let rho = 10f64.powf(-3.0 / (t60 * f)) as f32;
    let (di, frac) = (d.floor() as usize, (d - d.floor()) as f32);
    let at = |o: &[f32], i: usize| (1.0 - frac) * o[i - di] + frac * o[i - di - 1];
    for i in n..out.len() {
        let y = rho * 0.5 * (at(&out, i) + at(&out, i - 1));
        out[i] = y;
    }
    out.truncate(len);
    // a finger's attack takes a few ms
    let ramp = (0.004 * rate as f64) as usize;
    for (i, o) in out.iter_mut().take(ramp).enumerate() {
        *o *= i as f32 / ramp as f32;
    }
    normalise(&mut out, amp);
    out
}

/// Partials `parts` (amplitudes of harmonics 1, 2, ...) at `midi`, ramping up over `attack` seconds and decaying
/// `decay` per second (exponential rate, higher partials faster); its peak `amp`.
pub fn additive_pluck(
    midi: f64,
    rate: u32,
    secs: f64,
    amp: f32,
    parts: &[f32],
    attack: f64,
    decay: f64,
) -> Vec<f32> {
    let f = midi_to_hz(midi);
    let len = (secs * rate as f64) as usize;
    let mut out = vec![0.0f32; len];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f64 / rate as f64;
        let env = (t / attack.max(1e-4)).min(1.0);
        let mut v = 0.0;
        for (h, &p) in parts.iter().enumerate() {
            let k = (h + 1) as f64;
            if k * f > rate as f64 / 2.0 {
                break;
            }
            v += p as f64
                * (std::f64::consts::TAU * k * f * t + 0.3 * k).sin()
                * (-t * decay * (1.0 + 0.4 * h as f64)).exp();
        }
        *o = (env * v) as f32;
    }
    normalise(&mut out, amp);
    out
}

/// Adds `note` into `x` from sample `at` (cut at x's end).
pub fn mix_at(x: &mut [f32], at: usize, note: &[f32]) {
    for (i, &v) in note.iter().enumerate() {
        if let Some(o) = x.get_mut(at + i) {
            *o += v;
        }
    }
}

/// Seeded white noise at `rms`, `len` samples (a quiet input's hiss).
pub fn hiss(len: usize, rms: f32, seed: u32) -> Vec<f32> {
    let mut n = Noise::new(seed);
    // a uniform -1..1 has an RMS of 1/sqrt(3)
    (0..len).map(|_| n.sample() * rms * 3f32.sqrt()).collect()
}

fn normalise(x: &mut [f32], amp: f32) {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        for v in x.iter_mut() {
            *v *= amp / peak;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::mpm_pitch;
    use crate::spectrum::Spectra;

    #[test]
    fn the_string_is_in_tune_and_decays() {
        let rate = 48000;
        let mut sp = Spectra::new();
        for m in [23.0, 28.0, 40.0, 52.0] {
            let x = ks_pluck(m, rate, 2.0, 0.5, 0.6, 1);
            let (hz, _) = mpm_pitch(&mut sp, &x[9600..9600 + 8192], rate, 25.0, 1000.0, 0.95);
            let cents = 1200.0 * (hz / midi_to_hz(m)).log2();
            assert!(cents.abs() < 3.0, "{m}: {cents}");
            let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
            assert!(rms(&x[4800..9600]) > 1.5 * rms(&x[86400..91200]), "{m} decays");
        }
    }
}
