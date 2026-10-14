//! One monotonic clock for every thread and backend, in nanoseconds since the process started.
//!
//! Every audio block is stamped with the wall-clock time its first frame was captured (input) or will be heard
//! (output). The song clock maps those times to song time, so the scrolling chart, the song and the guitar onsets
//! line up whatever devices are in use (one duplex device, or two devices on different clocks).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Nanoseconds since the process started (monotonic; QueryPerformanceCounter on Windows).
pub fn now_ns() -> u64 {
    epoch().elapsed().as_nanos() as u64
}

/// An `Instant` as our nanosecond time (for backends that report Instants).
pub fn instant_ns(t: Instant) -> u64 {
    t.saturating_duration_since(epoch()).as_nanos() as u64
}

/// What the output side publishes every block: the song time heard at a wall-clock instant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SongClock {
    /// Wall time (ns) at which `song_time` is heard.
    pub wall_ns: u64,
    /// Song seconds (original tempo) heard at `wall_ns`.
    pub song_time: f64,
    /// Playback speed (song seconds per wall second); 0 while paused or buffering.
    pub rate: f64,
    /// Upper bound for extrapolation: the song time at the end of the last block rendered.
    pub limit: f64,
    pub playing: bool,
}

impl SongClock {
    /// Song time heard at wall time `t` (extrapolated from the last block, never past what was rendered).
    pub fn at(&self, t_ns: u64) -> f64 {
        if !self.playing || self.rate == 0.0 {
            return self.song_time;
        }
        let dt = (t_ns as f64 - self.wall_ns as f64) * 1e-9;
        (self.song_time + dt * self.rate).min(self.limit)
    }
}

/// A single-writer seqlock for the song clock: the audio thread writes it every block without waiting; readers
/// retry if they caught a write half-way.
#[derive(Default)]
pub struct ClockCell {
    seq: AtomicU32,
    wall_ns: AtomicU64,
    song_time: AtomicU64,
    rate: AtomicU64,
    limit: AtomicU64,
    playing: AtomicU32,
}

impl ClockCell {
    pub fn store(&self, c: &SongClock) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Relaxed);
        std::sync::atomic::fence(Ordering::Release);
        self.wall_ns.store(c.wall_ns, Ordering::Relaxed);
        self.song_time.store(c.song_time.to_bits(), Ordering::Relaxed);
        self.rate.store(c.rate.to_bits(), Ordering::Relaxed);
        self.limit.store(c.limit.to_bits(), Ordering::Relaxed);
        self.playing.store(c.playing as u32, Ordering::Relaxed);
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    pub fn load(&self) -> SongClock {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let c = SongClock {
                wall_ns: self.wall_ns.load(Ordering::Relaxed),
                song_time: f64::from_bits(self.song_time.load(Ordering::Relaxed)),
                rate: f64::from_bits(self.rate.load(Ordering::Relaxed)),
                limit: f64::from_bits(self.limit.load(Ordering::Relaxed)),
                playing: self.playing.load(Ordering::Relaxed) != 0,
            };
            std::sync::atomic::fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                return c;
            }
        }
    }
}

/// An f32 stored in an AtomicU32 (meters and gains shared with the audio threads).
#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        AtomicF32(AtomicU32::new(v.to_bits()))
    }
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_extrapolates_and_stops_at_limit() {
        let c = SongClock {
            wall_ns: 1_000_000_000,
            song_time: 10.0,
            rate: 1.0,
            limit: 10.5,
            playing: true,
        };
        assert!((c.at(1_200_000_000) - 10.2).abs() < 1e-9);
        assert!((c.at(3_000_000_000) - 10.5).abs() < 1e-9);
        let paused = SongClock { playing: false, ..c };
        assert_eq!(paused.at(9_000_000_000), 10.0);
    }

    #[test]
    fn seqlock_roundtrip() {
        let cell = ClockCell::default();
        let c = SongClock {
            wall_ns: 5,
            song_time: 1.5,
            rate: 0.5,
            limit: 2.0,
            playing: true,
        };
        cell.store(&c);
        assert_eq!(cell.load(), c);
    }

    #[test]
    fn now_is_monotonic() {
        let a = now_ns();
        let b = now_ns();
        assert!(b >= a);
    }
}
