//! Pluck detection on the guitar input. Port of rsplay/engine.py `_detect_onsets` (same thresholds): the signal's
//! RMS in hops of 256 samples; an onset is a hop clearly louder than the noise floor and than the hops just
//! before it, at least 50 ms after the previous onset. Runs on the input audio thread: no allocation.
//!
//! New in quickRS (D46): a strum on a chord that is still ringing (eighth-note rhythm) barely raises the RMS, so
//! the hop's high end (the RMS of the sample-to-sample difference, which a pick attack fills and a ringing string
//! soon loses) is also watched: a hop whose high end jumps well over the hops before it, at a level that is
//! clearly playing, is a pluck too.
//!
//! 2026-10-29 (owner's takes, D55): through an interface driven into clipping, or any compressed signal, a strum
//! raises neither the RMS nor the high end, and the live detector found 6-16 of 50-65 strums while the take
//! analysis (onsets.rs: spectral flux) found nearly all. So the same spectral flux runs here too, as the input
//! comes (`Flux`: 21 ms frames every 2.7 ms, a peak judged 16 ms later); a pluck either finds is one.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const HOP: usize = 256;
/// How far a hop's high end must jump over the hops before it to be a pluck on a ringing chord.
const HF_RISE: f32 = 2.0;

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
    /// The high end (RMS of the first difference): this hop's sum, the hops before, its floor, the last sample.
    hf_sum: f32,
    /// The previous hop's sum of squares.
    last_sum: f32,
    hf_hist: [f32; 4],
    hf_floor: f32,
    last_sample: f32,
    /// Minimum energy for an onset; raised for microphones (room noise). The calibration's pluck threshold.
    pub min_energy: f32,
    /// The loudest hop lately (decays over ~10 s): the threshold used is never above `PLAYING_SHARE` of it, so a
    /// calibration made at a louder input (the interface turned down since) can't hide every pluck.
    loud: f32,
    flux: Flux,
}

/// The threshold in use is at most this share of the loudest recent playing (-12 dB under it)...
const PLAYING_SHARE: f32 = 0.25;
/// ...and never under this (RMS, -56.5 dBFS: the lowest the calibration sets).
const LOUD_FLOOR: f32 = 0.0015;

/// Frames of flux kept (about 170 ms at 48 kHz): the peak's neighbourhood, the local mean and the recent maximum.
const FH: usize = 64;

/// Spectral flux of the log magnitude as the input comes (onsets.rs's novelty: 50 Hz-6 kHz, each bin against the
/// loudest of its neighbours a frame before). Its buffers are made in `new` (off the audio thread).
struct Flux {
    n: usize,
    hop: usize,
    fft: Arc<dyn RealToComplex<f32>>,
    win: Vec<f32>,
    /// The last n samples (a ring) and where the next goes.
    ring: Vec<f32>,
    pos: usize,
    filled: usize,
    since: usize,
    input: Vec<f32>,
    spec: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    prev: Vec<f32>,
    cur: Vec<f32>,
    lo: usize,
    hi: usize,
    /// Per frame (a ring of FH): its flux, its RMS, the capture index just after its last sample.
    flux: [f32; FH],
    rms: [f32; FH],
    end: [u64; FH],
    frames: u64,
    /// The strongest flux lately (decays by half in about 4 s): the peaks' threshold is a share of it.
    top: f32,
    decay: f32,
    w_peak: usize,
    w_mean: usize,
    w_recent: usize,
    min_gap: i64,
    last: i64,
    /// The fine energy (dB) in blocks of 64 samples, a ring of BLOCKS, and the block being summed: a pluck is
    /// moved to where it rises most (as onsets.rs's `Envelope::refine`).
    blk_db: [f32; BLOCKS],
    blk_end: [u64; BLOCKS],
    blk_n: u64,
    blk_sum: f32,
    blk_len: usize,
    /// Each frame's log spectrum (bins 0..=hi+1), a ring of FH: a pluck without a clear rise in level must change
    /// the spectrum for good (onsets.rs: "a new pitch must stay"), not ripple.
    specs: Vec<f32>,
    width: usize,
}

/// Fine-energy blocks kept (64 samples each: about 170 ms at 48 kHz).
const BLOCKS: usize = 128;
const BLK: usize = 64;

impl Flux {
    fn new(rate: u32) -> Flux {
        let n = ((rate as f64 * 0.0213) as usize).next_power_of_two().max(256);
        let hop = n / 8;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
        let binhz = rate as f32 / n as f32;
        let lo = ((50.0 / binhz) as usize).max(1);
        let hi = ((6000.0 / binhz) as usize).min(n / 2 - 1);
        let frames_in = |sec: f64| (sec * rate as f64 / hop as f64) as usize;
        let w_peak = frames_in(0.016).max(2);
        Flux {
            n,
            hop,
            win: (0..n)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
                .collect(),
            ring: vec![0.0; n],
            pos: 0,
            filled: 0,
            since: 0,
            input: fft.make_input_vec(),
            spec: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            prev: vec![0.0; hi + 2],
            cur: vec![0.0; hi + 2],
            lo,
            hi,
            fft,
            flux: [0.0; FH],
            rms: [0.0; FH],
            end: [0; FH],
            frames: 0,
            top: 0.0,
            decay: 0.5f32.powf(hop as f32 / rate as f32 / 4.0),
            w_peak,
            w_mean: frames_in(0.06).max(4),
            w_recent: frames_in(0.08).max(w_peak + 1),
            min_gap: (0.03 * rate as f64) as i64,
            last: i64::MIN / 2,
            blk_db: [-120.0; BLOCKS],
            blk_end: [0; BLOCKS],
            blk_n: 0,
            blk_sum: 0.0,
            blk_len: 0,
            specs: vec![0.0; FH * (hi + 2)],
            width: hi + 2,
        }
    }

    /// The ring slot of the frame `back` frames before the newest.
    fn at(&self, back: usize) -> usize {
        ((self.frames as usize).wrapping_sub(1 + back)) % FH
    }

    /// One sample in (`idx` its capture index); a pluck's capture index when one is decided.
    fn push(&mut self, x: f32, idx: u64, min_energy: f32) -> Option<u64> {
        self.blk_sum += x * x;
        self.blk_len += 1;
        if self.blk_len == BLK {
            let slot = (self.blk_n as usize) % BLOCKS;
            self.blk_db[slot] = 10.0 * (self.blk_sum / BLK as f32 + 1e-12).log10();
            self.blk_end[slot] = idx + 1;
            self.blk_n += 1;
            self.blk_sum = 0.0;
            self.blk_len = 0;
        }
        self.ring[self.pos] = x;
        self.pos = (self.pos + 1) % self.n;
        self.filled = (self.filled + 1).min(self.n);
        self.since += 1;
        if self.since < self.hop || self.filled < self.n {
            return None;
        }
        self.since = 0;
        let mut e = 0.0f32;
        for i in 0..self.n {
            let v = self.ring[(self.pos + i) % self.n];
            e += v * v;
            self.input[i] = v * self.win[i];
        }
        let _ = self
            .fft
            .process_with_scratch(&mut self.input, &mut self.spec, &mut self.scratch);
        for k in self.lo - 1..=self.hi + 1 {
            self.cur[k] = (1.0 + self.spec[k].norm()).ln();
        }
        let mut d = 0.0f32;
        if self.frames > 0 {
            for k in self.lo..=self.hi {
                let before = self.prev[k - 1].max(self.prev[k]).max(self.prev[k + 1]);
                d += (self.cur[k] - before).max(0.0);
            }
        }
        let slot = (self.frames as usize) % FH;
        self.specs[slot * self.width..(slot + 1) * self.width].copy_from_slice(&self.cur);
        std::mem::swap(&mut self.prev, &mut self.cur);
        self.flux[slot] = d;
        self.rms[slot] = (e / self.n as f32).sqrt();
        self.end[slot] = idx + 1;
        self.frames += 1;
        self.top = (self.top * self.decay).max(d);
        self.decide(min_energy)
    }

    /// Is the frame w_peak back a pluck? (Its neighbourhood on both sides is in.)
    fn decide(&mut self, min_energy: f32) -> Option<u64> {
        let (wp, wm, wr) = (self.w_peak, self.w_mean, self.w_recent);
        if (self.frames as usize) < wp + wr + 2 {
            return None;
        }
        let c = wp; // frames back
        let v = self.flux[self.at(c)];
        let delta = 0.08 * self.top;
        if v < delta {
            return None;
        }
        // the highest point within w_peak either way (the first, if level)
        for b in 0..=2 * wp {
            let u = self.flux[self.at(b)];
            if u > v || (b > c && u >= v) {
                return None;
            }
        }
        let local = (c + 1..=c + wm).map(|b| self.flux[self.at(b)]).sum::<f32>() / wm as f32;
        if v < local + delta {
            return None;
        }
        // where the flux first got to half this peak
        let mut g = c;
        while g < c + wp && self.flux[self.at(g + 1)] >= 0.5 * v {
            g += 1;
        }
        let after = (0..=c).map(|b| self.rms[self.at(b)]).fold(0.0f32, f32::max);
        let before = self.rms[self.at(g + 3)];
        let rise = after / before.max(1e-6);
        // standing out from the last 80 ms's ripple (a chord's partials beating), unless the level clearly rose
        let recent = (c + wp + 1..=c + wr)
            .map(|b| self.flux[self.at(b)])
            .fold(0.0f32, f32::max);
        if v < 1.25 * recent && rise < 2.0 {
            return None;
        }
        // playing, and not a note dying away
        if after < min_energy || after < 0.6 * before {
            return None;
        }
        // no clear rise in level: what came in must stay (the newest frame, 16 ms on, against a frame wholly
        // before the rise), or it was the chord's partials beating
        if rise < 1.4 {
            let pre = self.at(g + self.n / self.hop + 1) * self.width;
            let post = self.at(0) * self.width;
            let mut d = 0.0f32;
            for k in self.lo..=self.hi {
                let before = self.specs[pre + k - 1]
                    .max(self.specs[pre + k])
                    .max(self.specs[pre + k + 1]);
                d += (self.specs[post + k] - before).max(0.0);
            }
            if d < 0.3 * v {
                return None;
            }
        }
        // the pluck: where the fine energy rises most in the frame where the flux started rising (else near its end)
        let (f_end, f_start) = (
            self.end[self.at(g)],
            self.end[self.at(g)].saturating_sub(self.n as u64),
        );
        let mut idx = f_end as i64 - self.hop as i64;
        let mut best = 3.0f32;
        let back = (self.blk_n as usize).min(BLOCKS);
        for b in 2..back {
            let k = ((self.blk_n as usize) - 1 - b + BLOCKS * 4) % BLOCKS;
            let end = self.blk_end[k];
            if end <= f_start {
                break;
            }
            if end > f_end + BLK as u64 {
                continue;
            }
            let k2 = (k + BLOCKS - 2) % BLOCKS;
            let rise = self.blk_db[k] - self.blk_db[k2];
            if rise > best {
                best = rise;
                idx = end as i64 - BLK as i64;
            }
        }
        if idx - self.last < self.min_gap {
            return None;
        }
        self.last = idx;
        Some(idx.max(0) as u64)
    }
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
            hf_sum: 0.0,
            last_sum: 0.0,
            hf_hist: [0.0; 4],
            hf_floor: 0.0005,
            last_sample: 0.0,
            min_energy: 0.004,
            loud: 0.0,
            flux: Flux::new(rate),
        }
    }

    pub fn reset(&mut self) {
        self.hop_len = 0;
        self.hf_sum = 0.0;
    }

    /// Feed samples starting at capture index `start_idx`; `emit(index, energy)` for each onset found.
    pub fn feed(&mut self, x: &[f32], start_idx: u64, mut emit: impl FnMut(u64, f32)) {
        if self.hop_len == 0 {
            self.hop_start = start_idx;
        }
        let min_gap = (0.05 * self.rate as f64) as i64;
        for (k, &s) in x.iter().enumerate() {
            // the spectral flux's plucks (decided 16 ms on), unless one was just found
            let gate = self.min_energy.min((PLAYING_SHARE * self.loud).max(LOUD_FLOOR));
            if let Some(i) = self.flux.push(s, start_idx + k as u64, gate)
                && i as i64 - self.last_onset > min_gap
            {
                self.last_onset = i as i64;
                let e = self.flux.rms[self.flux.at(0)];
                emit(i, e);
            }
            self.hop[self.hop_len] = s;
            self.hop_len += 1;
            let d = s - self.last_sample;
            self.last_sample = s;
            self.hf_sum += d * d;
            if self.hop_len < HOP {
                continue;
            }
            let sum = self.hop.iter().map(|v| v * v).sum::<f32>();
            let e = (sum / HOP as f32).sqrt();
            // (the loudest lately: down 60 dB over ~10 s of hops)
            let decay = 10f32.powf(-3.0 * HOP as f32 / (10.0 * self.rate as f32));
            self.loud = (self.loud * decay).max(e);
            // the rise is judged on this hop and the one before (D46: a hop is shorter than a low string's period,
            // so a ringing chord's hop RMS swings by 2-4x and a single hop used to make plucks of its own)
            let e2 = ((sum + self.last_sum) / (2 * HOP) as f32).sqrt();
            self.last_sum = sum;
            let prev = self.hist.iter().sum::<f32>() / self.hist.len() as f32;
            let eh = (self.hf_sum / HOP as f32).sqrt();
            self.hf_sum = 0.0;
            let hf_prev = self.hf_hist.iter().sum::<f32>() / self.hf_hist.len() as f32;
            let idx = self.hop_start as i64;
            // the noise floor: down at once, up slowly and only from hops near it (D46: it used to creep up to the
            // playing level in a long rhythm part with no gaps, about 10 s in, and then no strum was loud enough)
            if e < self.floor {
                self.floor = e.max(1e-5);
            } else if e < self.floor * 1.5 {
                self.floor += (e - self.floor) * 0.002;
            }
            if eh < self.hf_floor {
                self.hf_floor = eh.max(1e-6);
            } else if eh < self.hf_floor * 1.5 {
                self.hf_floor += (eh - self.hf_floor) * 0.002;
            }
            let loud = e > self.min_energy.max(self.floor * 4.0);
            let rose = e2 > prev * 1.6;
            let attack = eh > hf_prev * HF_RISE && eh > self.hf_floor * 8.0;
            if loud && (rose || attack) && idx - self.last_onset > min_gap {
                self.last_onset = idx;
                emit(idx as u64, e);
            }
            self.hist[self.hist_pos] = e2;
            self.hf_hist[self.hist_pos] = eh;
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

    /// The calibration made at a louder input (threshold 0.03 RMS) and the guitar now 30 dB quieter (the owner's
    /// take of 2026-11-03: its loudest 10 ms 0.02 RMS): the plucks are still found, the noise still isn't.
    #[test]
    fn a_stale_calibration_does_not_hide_quiet_plucks() {
        let rate = 48000;
        let mut d = OnsetDetector::new(rate);
        d.min_energy = 0.03;
        let mut x = vec![0.0002f32; 2 * rate as usize];
        for k in 0..x.len() {
            x[k] *= if k % 2 == 0 { 1.0 } else { -1.0 };
        }
        let at = [12000usize, 36000, 60000, 84000];
        for &a in &at {
            for i in 0..9000 {
                x[a + i] += 0.035 * (-(i as f32) / 2500.0).exp() * ((i as f32) * 0.03).sin();
            }
        }
        let mut found = Vec::new();
        d.feed(&x, 0, |idx, _| found.push(idx));
        assert_eq!(found.len(), at.len(), "{found:?}");
        for (f, a) in found.iter().zip(at) {
            assert!((*f as i64 - a as i64).abs() < 400, "{f} vs {a}");
        }
    }

    /// A strummed chord (six decaying strings, a bright pick attack), `n` strums `gap` seconds apart from 0.5 s,
    /// each cutting the one before.
    fn strums(rate: u32, n: usize, gap: f64) -> (Vec<f32>, Vec<usize>) {
        let len = ((0.5 + n as f64 * gap + 1.0) * rate as f64) as usize;
        let mut x = vec![0.0f32; len];
        let mut at = Vec::new();
        let fs = [82.4f32, 123.5, 164.8, 207.7, 246.9, 329.6];
        for k in 0..n {
            let a = ((0.5 + k as f64 * gap) * rate as f64) as usize;
            let next = ((0.5 + (k + 1) as f64 * gap) * rate as f64) as usize;
            at.push(a);
            for (j, f) in fs.iter().enumerate() {
                // 5 ms a string; it rings until the next strum strikes it again (a 5 ms fade)
                let s0 = a + j * rate as usize / 200;
                let end = if k + 1 < n {
                    next + j * rate as usize / 200
                } else {
                    len
                };
                let fade = rate as usize / 200;
                let mut seed = ((k * 7 + j) as u32).wrapping_mul(2654435761);
                for i in s0..(end + fade).min(len) {
                    let t = (i - s0) as f32 / rate as f32;
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let click = if t < 0.003 {
                        (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5
                    } else {
                        0.0
                    };
                    let w = std::f32::consts::TAU * f * t;
                    let v = (w.sin() + 0.3 * (3.0 * w).sin() * (-t * 20.0).exp()) * (-t * 1.5).exp();
                    let g = if i >= end {
                        1.0 - (i - end) as f32 / fade as f32
                    } else {
                        1.0
                    };
                    x[i] += (0.04 * v + 0.03 * click) * g;
                }
            }
        }
        (x, at)
    }

    /// D46: eighth-note strums of a ringing chord (the RMS hardly rises) are found by their pick attacks, and still
    /// after 20 s of strumming with no gap (the noise floor used to creep up to the playing level and stop them).
    #[test]
    fn rhythm_strums_are_found_all_the_way() {
        let rate = 48000;
        let (x, at) = strums(rate, 104, 0.19);
        let mut found = Vec::new();
        let mut d = OnsetDetector::new(rate);
        for (k, c) in x.chunks(128).enumerate() {
            d.feed(c, (k * 128) as u64, |i, _| found.push(i as usize));
        }
        // (within 30 ms: the strum's own window still holds it)
        let hit = at
            .iter()
            .filter(|&&a| found.iter().any(|&f| f + 1500 >= a && f <= a + 1500))
            .count();
        assert!(hit >= at.len() - 2, "{hit} of {} strums found", at.len());
        let late = at[at.len() - 10..]
            .iter()
            .filter(|&&a| found.iter().any(|&f| f + 1500 >= a && f <= a + 1500))
            .count();
        assert_eq!(late, 10, "the last strums, 19 s in");
        assert!(
            found.len() <= at.len() + 4,
            "{} plucks for {} strums",
            found.len(),
            at.len()
        );
    }

    /// D55 (the owner's takes): strums through an interface driven into clipping (here hard-clipped at a third of
    /// their peak) are still found, by the spectral flux. Known: as each chord decays out of clipping its waveform
    /// changes shape and the flux makes one extra pluck between strums (a pluck that no chart group wants costs
    /// nothing; the owner's real clipped takes showed few).
    #[test]
    fn clipped_strums_are_found() {
        let rate = 48000;
        let (mut x, at) = strums(rate, 60, 0.19);
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        for v in &mut x {
            *v = v.clamp(-peak / 3.0, peak / 3.0) * 3.0 / peak;
        }
        let mut found = Vec::new();
        let mut d = OnsetDetector::new(rate);
        for (k, c) in x.chunks(128).enumerate() {
            d.feed(c, (k * 128) as u64, |i, _| found.push(i as usize));
        }
        let hit = at
            .iter()
            .filter(|&&a| found.iter().any(|&f| f + 1500 >= a && f <= a + 1500))
            .count();
        assert!(hit >= at.len() - 2, "{hit} of {} strums found", at.len());
        assert!(
            found.len() <= 2 * at.len(),
            "{} plucks for {} strums",
            found.len(),
            at.len()
        );
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

/// P32 (bass mode, D89): a bass played with the fingers, synthetic (no bass here to record): Karplus-Strong strings
/// and soft additive notes whose attack takes 15 ms, down to a 5-string's low B (31 Hz), at -46 to -10 dBFS peaks.
/// The detector as it is finds them (a bass test of its own was tried and only added false plucks: D89).
#[cfg(test)]
mod bass_tests {
    use super::*;
    use qrs_analysis::testsig;

    /// `n` notes `gap` apart on one string (root, root, minor third, fourth), each stopped by the next (a 5 ms
    /// crossfade, as a re-plucked string), the last ringing on; in hiss at -70 dBFS. (found within 30 ms, plucks)
    fn line(midi: f64, amp: f32, soft: Option<f32>, gap: f64) -> (usize, usize, usize) {
        let rate = 48000u32;
        let n = 12;
        let len = ((1.0 + n as f64 * gap + 2.5) * rate as f64) as usize;
        let mut x = testsig::hiss(len, 0.0003, 9);
        let mut at = vec![];
        for k in 0..=n {
            let a = ((1.0 + k as f64 * gap) * rate as f64) as usize;
            at.push(a);
            let m = midi + [0.0, 0.0, 3.0, 5.0][k % 4];
            let secs = if k == n { 2.0 } else { gap + 0.005 };
            let mut note = match soft {
                Some(s) => testsig::ks_pluck(m, rate, secs, amp, s, k as u32),
                None => testsig::additive_pluck(m, rate, secs, amp, &[1.0, 0.5, 0.2, 0.05], 0.015, 1.0),
            };
            if k < n {
                let (l, fade) = (note.len(), rate as usize / 200);
                for i in 0..fade {
                    note[l - 1 - i] *= i as f32 / fade as f32;
                }
            }
            testsig::mix_at(&mut x, a, &note);
        }
        let mut found = Vec::new();
        let mut d = OnsetDetector::new(rate);
        d.min_energy = 0.002;
        for (k, c) in x.chunks(128).enumerate() {
            d.feed(c, (k * 128) as u64, |i, _| found.push(i as usize));
        }
        let hit = at
            .iter()
            .filter(|&&a| found.iter().any(|&f| f + 1440 >= a && f <= a + 1440))
            .count();
        (hit, at.len(), found.len())
    }

    #[test]
    fn finger_plucked_bass_lines_are_found() {
        let mut total = 0;
        for (midi, amp, soft, gap) in [
            (28.0, 0.05, Some(0.7), 0.5),  // E1
            (23.0, 0.05, Some(0.7), 0.5),  // B0
            (23.0, 0.01, Some(0.8), 0.5),  // B0, quiet
            (28.0, 0.3, Some(0.6), 0.25),  // E1, loud eighths at 120
            (33.0, 0.02, Some(0.85), 0.3), // A1
            (40.0, 0.02, Some(0.85), 0.3), // E2
            (28.0, 0.02, None, 0.3),       // E1, a slow attack
            (23.0, 0.02, None, 0.3),       // B0, a slow attack
            (23.0, 0.005, None, 0.3),      // B0, a slow attack at -46 dBFS
        ] {
            let (hit, of, plucks) = line(midi, amp, soft, gap);
            // 2026-11-04: all found but 2 of the softest A1's 13 (the same note plucked again, its level hardly
            // rising): 115 of 117 in all
            assert!(hit + 2 >= of, "{midi} {amp} {soft:?}: {hit} of {of}");
            total += hit;
            // (a slow B0 makes up to four extra: a pluck no chart note wants costs nothing)
            assert!(
                plucks <= of + 4,
                "{midi} {amp} {soft:?}: {plucks} plucks for {of}"
            );
        }
        assert!(total >= 115, "{total} of 117");
    }
}
