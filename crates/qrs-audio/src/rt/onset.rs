//! Pluck detection on the guitar input. Port of rsplay/engine.py `_detect_onsets` (same thresholds): the signal's
//! RMS in hops of 256 samples; an onset is a hop clearly louder than the noise floor and than the hops just
//! before it, at least 50 ms after the previous onset. Runs on the input audio thread: no allocation.

pub const HOP: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Onset {
    /// Capture-ring index of the hop where the pluck was found.
    pub index: u64,
    /// Wall time (ns) that sample was captured.
    pub wall_ns: u64,
    /// The song time heard when the guitar was plucked (None while the song isn't playing).
    pub song_time: Option<f64>,
    /// The hop's RMS.
    pub energy: f32,
}

pub struct OnsetDetector {
    rate: u32,
    hop: [f32; HOP],
    hop_len: usize,
    /// Capture index of hop[0].
    hop_start: u64,
    hist: [f32; 4],
    hist_pos: usize,
    floor: f32,
    last_onset: i64,
    /// Minimum energy for an onset; raised for microphones (room noise).
    pub min_energy: f32,
}

impl OnsetDetector {
    pub fn new(rate: u32) -> OnsetDetector {
        OnsetDetector {
            rate,
            hop: [0.0; HOP],
            hop_len: 0,
            hop_start: 0,
            hist: [0.0; 4],
            hist_pos: 0,
            floor: 0.002,
            last_onset: -1_000_000_000,
            min_energy: 0.004,
        }
    }

    pub fn reset(&mut self) {
        self.hop_len = 0;
    }

    /// Feed samples starting at capture index `start_idx`; `emit(index, energy)` for each onset found.
    pub fn feed(&mut self, x: &[f32], start_idx: u64, mut emit: impl FnMut(u64, f32)) {
        if self.hop_len == 0 {
            self.hop_start = start_idx;
        }
        let min_gap = (0.05 * self.rate as f64) as i64;
        for &s in x {
            self.hop[self.hop_len] = s;
            self.hop_len += 1;
            if self.hop_len < HOP {
                continue;
            }
            let e = (self.hop.iter().map(|v| v * v).sum::<f32>() / HOP as f32).sqrt();
            let prev = self.hist.iter().sum::<f32>() / self.hist.len() as f32;
            let idx = self.hop_start as i64;
            if e < self.floor {
                self.floor = e.max(1e-5);
            } else {
                self.floor += (e - self.floor) * 0.0005;
            }
            if e > self.min_energy.max(self.floor * 4.0) && e > prev * 1.6 && idx - self.last_onset > min_gap
            {
                self.last_onset = idx;
                emit(idx as u64, e);
            }
            self.hist[self.hist_pos] = e;
            self.hist_pos = (self.hist_pos + 1) % self.hist.len();
            self.hop_len = 0;
            self.hop_start += HOP as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_plucks_not_noise() {
        let rate = 48000;
        let mut d = OnsetDetector::new(rate);
        let mut x = vec![0.0005f32; rate as usize]; // quiet noise floor
        for k in 0..x.len() {
            x[k] *= if k % 2 == 0 { 1.0 } else { -1.0 };
        }
        // two plucks: decaying bursts at 0.25 s and 0.6 s
        for &at in &[12000usize, 28800] {
            for i in 0..6000 {
                x[at + i] += 0.5 * (-(i as f32) / 1500.0).exp() * ((i as f32) * 0.06).sin();
            }
        }
        let mut found = Vec::new();
        d.feed(&x, 0, |idx, _| found.push(idx));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!((found[0] as i64 - 12000).abs() < 300);
        assert!((found[1] as i64 - 28800).abs() < 300);
    }

    #[test]
    fn split_feeds_give_the_same_indices() {
        let mut x = vec![0.0f32; 20000];
        for i in 0..4000 {
            x[8000 + i] = 0.3 * ((i as f32) * 0.05).sin();
        }
        let mut a = Vec::new();
        OnsetDetector::new(48000).feed(&x, 100, |i, _| a.push(i));
        let mut b = Vec::new();
        let mut d = OnsetDetector::new(48000);
        let mut pos = 100;
        for chunk in x.chunks(97) {
            d.feed(chunk, pos, |i, _| b.push(i));
            pos += chunk.len() as u64;
        }
        assert_eq!(a, b);
        assert_eq!(a.len(), 1);
    }
}
