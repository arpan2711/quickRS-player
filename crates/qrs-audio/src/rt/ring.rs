//! The capture ring: the last few seconds of the guitar input, written by the input audio thread and read by any
//! number of other threads (tuner, scoring, note widget). Samples are addressed by their absolute index since the
//! engine started, so a reader can ask for "the 4096 samples from index N" and know whether they are still there.
//!
//! Lock-free and data-race free: samples live in `AtomicU32`s (plain loads and stores on x86 and ARM), the writer
//! publishes the new total with Release, readers copy and then re-check that nothing they copied was overwritten.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub struct CaptureRing {
    buf: Box<[AtomicU32]>,
    mask: u64,
    written: AtomicU64,
    /// Sample rate of what's recorded (the input device's).
    rate: AtomicU32,
}

impl CaptureRing {
    /// A ring holding 2^bits samples.
    pub fn new(bits: u32) -> CaptureRing {
        let n = 1usize << bits;
        CaptureRing {
            buf: (0..n).map(|_| AtomicU32::new(0)).collect(),
            mask: n as u64 - 1,
            written: AtomicU64::new(0),
            rate: AtomicU32::new(48000),
        }
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Total samples written since the start (the index the next sample will get).
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }

    pub fn rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed)
    }

    pub fn set_rate(&self, r: u32) {
        self.rate.store(r, Ordering::Relaxed)
    }

    /// Samples a writer may be overwriting before it publishes them: readers treat the oldest `guard` samples of
    /// the window as gone, and writes are published in pieces no longer than this.
    fn guard(&self) -> u64 {
        (self.buf.len() as u64 / 4).min(16384)
    }

    /// Audio thread only (single writer).
    pub fn write(&self, x: &[f32]) {
        let skip = x.len().saturating_sub(self.buf.len());
        let mut w = self.written.load(Ordering::Relaxed) + skip as u64;
        for piece in x[skip..].chunks(self.guard() as usize) {
            for (i, s) in piece.iter().enumerate() {
                self.buf[((w + i as u64) & self.mask) as usize].store(s.to_bits(), Ordering::Relaxed);
            }
            w += piece.len() as u64;
            self.written.store(w, Ordering::Release);
        }
    }

    /// How many past samples can be read back reliably.
    pub fn window(&self) -> u64 {
        self.buf.len() as u64 - self.guard()
    }

    /// Samples [start, start + out.len()), or false if they're not recorded yet or already overwritten.
    pub fn read(&self, start: u64, out: &mut [f32]) -> bool {
        let n = out.len() as u64;
        let w = self.written();
        if start + n > w || start + self.window() < w {
            return false;
        }
        for (i, o) in out.iter_mut().enumerate() {
            *o = f32::from_bits(self.buf[((start + i as u64) & self.mask) as usize].load(Ordering::Relaxed));
        }
        std::sync::atomic::fence(Ordering::Acquire);
        // the writer may have lapped us while we copied (it never writes more than `guard` ahead of `written`)
        let w2 = self.written.load(Ordering::Relaxed);
        start + self.window() >= w2
    }

    /// The latest `out.len()` samples; returns the index of the first one, or None if not enough are recorded.
    pub fn latest(&self, out: &mut [f32]) -> Option<u64> {
        for _ in 0..4 {
            let w = self.written();
            if w < out.len() as u64 {
                return None;
            }
            let start = w - out.len() as u64;
            if self.read(start, out) {
                return Some(start);
            }
        }
        None
    }
}

impl qrs_analysis::SampleSource for CaptureRing {
    fn rate(&self) -> u32 {
        CaptureRing::rate(self)
    }
    fn written(&self) -> u64 {
        CaptureRing::written(self)
    }
    fn read(&self, start: u64, out: &mut [f32]) -> bool {
        CaptureRing::read(self, start, out)
    }
    fn latest(&self, out: &mut [f32]) -> bool {
        CaptureRing::latest(self, out).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_latest_and_overwrite() {
        let r = CaptureRing::new(4); // 16 samples, guard 4: the last 12 can be read back
        let x: Vec<f32> = (0..10).map(|i| i as f32).collect();
        r.write(&x);
        let mut out = [0.0; 4];
        assert_eq!(r.latest(&mut out), Some(6));
        assert_eq!(out, [6.0, 7.0, 8.0, 9.0]);
        assert!(r.read(0, &mut out));
        r.write(&x); // 20 written: indices before 8 are gone
        assert!(!r.read(4, &mut out));
        assert!(r.read(8, &mut out));
        assert_eq!(out, [8.0, 9.0, 0.0, 1.0]);
        assert!(!r.read(18, &mut out), "not written yet");
    }

    #[test]
    fn huge_write_keeps_the_tail() {
        let r = CaptureRing::new(3); // 8 samples, guard 2, window 6
        let x: Vec<f32> = (0..20).map(|i| i as f32).collect();
        r.write(&x);
        let mut out = [0.0; 6];
        assert_eq!(r.latest(&mut out), Some(14));
        assert_eq!(out[5], 19.0);
    }
}
