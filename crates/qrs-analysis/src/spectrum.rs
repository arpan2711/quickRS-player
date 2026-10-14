//! Magnitude spectra with cached FFT plans (numpy's `abs(rfft(x * hanning(n), nfft))`).

use std::collections::HashMap;
use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

/// FFT plans and scratch, reused between calls (one per thread).
#[derive(Default)]
pub struct Spectra {
    planner: RealFftPlanner<f64>,
    plans: HashMap<usize, Arc<dyn RealToComplex<f64>>>,
    windows: HashMap<usize, Vec<f64>>,
}

impl Spectra {
    pub fn new() -> Spectra {
        Spectra::default()
    }

    fn plan(&mut self, n: usize) -> Arc<dyn RealToComplex<f64>> {
        if let Some(p) = self.plans.get(&n) {
            return p.clone();
        }
        let p = self.planner.plan_fft_forward(n);
        self.plans.insert(n, p.clone());
        p
    }

    /// numpy's symmetric Hann window.
    fn window(&mut self, n: usize) -> &[f64] {
        self.windows.entry(n).or_insert_with(|| {
            if n <= 1 {
                return vec![1.0; n];
            }
            (0..n)
                .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos())
                .collect()
        })
    }

    /// (magnitudes, Hz per bin) of a Hann-windowed frame, zero-padded to at least 4x its length (and 16384).
    pub fn spectrum(&mut self, x: &[f32], rate: u32) -> (Vec<f64>, f64) {
        let n = x.len();
        let bits = ((n * 4).max(1) as f64).log2().ceil().max(14.0) as u32;
        let nfft = 1usize << bits;
        let plan = self.plan(nfft);
        let w = self.window(n).to_vec();
        let mut input = vec![0.0f64; nfft];
        for i in 0..n {
            input[i] = x[i] as f64 * w[i];
        }
        let mut out = plan.make_output_vec();
        let _ = plan.process(&mut input, &mut out);
        (out.iter().map(|c| c.norm()).collect(), rate as f64 / nfft as f64)
    }

    /// The real FFT of `x` zero-padded to `n` (for autocorrelation).
    pub fn rfft(&mut self, x: &[f64], n: usize) -> Vec<realfft::num_complex::Complex<f64>> {
        let plan = self.plan(n);
        let mut input = vec![0.0f64; n];
        input[..x.len().min(n)].copy_from_slice(&x[..x.len().min(n)]);
        let mut out = plan.make_output_vec();
        let _ = plan.process(&mut input, &mut out);
        out
    }

    pub fn planner(&mut self) -> &mut RealFftPlanner<f64> {
        &mut self.planner
    }
}

/// numpy's median.
pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}
