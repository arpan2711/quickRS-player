//! Pitch: the McLeod Pitch Method (the tuner and the note readout) and a harmonic-product-spectrum estimate.
//! Port of rsplay/detect.py `mpm_pitch` / `estimate_pitch`.

use crate::spectrum::Spectra;

pub fn midi_to_hz(m: f64) -> f64 {
    440.0 * 2f64.powf((m - 69.0) / 12.0)
}

pub fn hz_to_midi(f: f64) -> f64 {
    69.0 + 12.0 * (f / 440.0).log2()
}

/// "E2", "C#4"
pub fn note_label(midi: f64) -> String {
    let m = midi.round() as i32;
    format!("{}{}", qrs_formats::model::note_name(m), m.div_euclid(12) - 1)
}

/// McLeod Pitch Method: (frequency Hz, clarity 0..1); frequency 0 when nothing periodic is found.
///
/// Normalised square difference function via FFT autocorrelation, then the first peak that reaches k times the
/// tallest one (avoids octave errors), refined with parabolic interpolation. About a cent on a held guitar note.
pub fn mpm_pitch(sp: &mut Spectra, x: &[f32], rate: u32, fmin: f64, fmax: f64, k: f64) -> (f64, f64) {
    let n = x.len();
    if n < 8 {
        return (0.0, 0.0);
    }
    let mean = x.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let xd: Vec<f64> = x.iter().map(|&v| v as f64 - mean).collect();
    // autocorrelation by FFT of size 2n (numpy: irfft(spec * conj(spec)) with n = 2n)
    let m2 = 2 * n;
    let spec = sp.rfft(&xd, m2);
    let mut power: Vec<realfft::num_complex::Complex<f64>> = spec.iter().map(|c| c * c.conj()).collect();
    let inv = sp.planner().plan_fft_inverse(m2);
    let mut acf_full = inv.make_output_vec();
    // the DC and Nyquist bins of a real spectrum must be real
    if let Some(first) = power.first_mut() {
        first.im = 0.0;
    }
    if let Some(last) = power.last_mut() {
        last.im = 0.0;
    }
    let _ = inv.process(&mut power, &mut acf_full);
    let acf: Vec<f64> = acf_full[..n].iter().map(|v| v / m2 as f64).collect();
    let mut sq = vec![0.0f64; n + 1];
    for i in 0..n {
        sq[i + 1] = sq[i] + xd[i] * xd[i];
    }
    let nsdf: Vec<f64> = (0..n)
        .map(|tau| {
            let m = sq[n - tau] + (sq[n] - sq[tau]);
            if m > 0.0 {
                2.0 * acf[tau] / m.max(1e-12)
            } else {
                0.0
            }
        })
        .collect();
    let lo = ((rate as f64 / fmax) as usize).max(2);
    let hi = ((rate as f64 / fmin) as usize).min(n - 2);
    let mut tau = 1;
    while tau < hi && nsdf[tau] > 0.0 {
        tau += 1; // leave the main lobe
    }
    let mut peaks: Vec<(usize, f64)> = Vec::new();
    while tau < hi {
        while tau < hi && nsdf[tau] <= 0.0 {
            tau += 1;
        }
        let (mut best, mut best_tau) = (-1.0, usize::MAX);
        while tau < hi && nsdf[tau] > 0.0 {
            if nsdf[tau] > best {
                best = nsdf[tau];
                best_tau = tau;
            }
            tau += 1;
        }
        if best_tau != usize::MAX && best_tau >= lo {
            peaks.push((best_tau, best));
        }
    }
    if peaks.is_empty() {
        return (0.0, 0.0);
    }
    let top = peaks.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let &(t, clarity) = peaks.iter().find(|p| p.1 >= k * top).unwrap();
    let (a, b, c) = (nsdf[t - 1], nsdf[t], nsdf[t + 1]);
    let d = a - 2.0 * b + c;
    let shift = if d != 0.0 { 0.5 * (a - c) / d } else { 0.0 };
    (rate as f64 / (t as f64 + shift), clarity)
}

/// MPM with the defaults the app uses (35..1400 Hz, k = 0.9).
pub fn pitch(sp: &mut Spectra, x: &[f32], rate: u32) -> (f64, f64) {
    mpm_pitch(sp, x, rate, 35.0, 1400.0, 0.9)
}

/// Rough monophonic pitch (MIDI number) by harmonic product spectrum.
pub fn estimate_pitch(sp: &mut Spectra, x: &[f32], rate: u32) -> Option<f64> {
    let (mag, binhz) = sp.spectrum(x, rate);
    let n = mag.len() / 3;
    let hps: Vec<f64> = (0..n).map(|i| mag[i] * mag[2 * i] * mag[3 * i]).collect();
    let lo = (35.0 / binhz) as usize;
    let hi = (n.saturating_sub(1)).min((1400.0 / binhz) as usize);
    if hi <= lo {
        return None;
    }
    let mut k = lo;
    for i in lo..hi {
        if hps[i] > hps[k] {
            k = i;
        }
    }
    if hps[k] <= 0.0 {
        return None;
    }
    let mut kf = k as f64;
    if k > 0 && k < mag.len() - 1 {
        let (a, b, c) = (
            (mag[k - 1] + 1e-12).ln(),
            (mag[k] + 1e-12).ln(),
            (mag[k + 1] + 1e-12).ln(),
        );
        let d = a - 2.0 * b + c;
        kf += if d != 0.0 { 0.5 * (a - c) / d } else { 0.0 };
    }
    let f = kf * binhz;
    (f > 0.0).then(|| hz_to_midi(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pluck(f: f64, rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f64 / rate as f64;
                let w = std::f64::consts::TAU * f * t;
                (0.5 * (w.sin() + 0.6 * (2.0 * w).sin() + 0.3 * (3.0 * w).sin()) * (-t * 2.0).exp()) as f32
            })
            .collect()
    }

    #[test]
    fn mpm_is_accurate_to_a_cent() {
        let mut sp = Spectra::new();
        for f in [82.41, 110.0, 196.0, 329.63, 41.2] {
            let (got, clarity) = pitch(&mut sp, &pluck(f, 48000, 4096), 48000);
            let cents = 1200.0 * (got / f).log2();
            assert!(cents.abs() < 2.0, "{f}: {got} ({cents} cents)");
            assert!(clarity > 0.8);
        }
    }

    #[test]
    fn noise_has_low_clarity() {
        let mut x: u32 = 7;
        let noise: Vec<f32> = (0..4096)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 8) as f32 / 16777216.0 - 0.5
            })
            .collect();
        let mut sp = Spectra::new();
        let (_, clarity) = pitch(&mut sp, &noise, 48000);
        assert!(clarity < 0.6, "{clarity}");
    }

    #[test]
    fn hps_estimate() {
        let mut sp = Spectra::new();
        let m = estimate_pitch(&mut sp, &pluck(110.0, 48000, 8192), 48000).unwrap();
        assert!((m - 45.0).abs() < 0.3, "{m}");
        assert_eq!(note_label(40.0), "E2");
        assert_eq!(note_label(61.0), "C#4");
    }
}
