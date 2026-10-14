//! The real-time core: what runs inside the audio callbacks, independent of any device API.
//!
//! - `InputSide` (input callback): channel pick, meters, the capture ring, onset detection, the tone, and the
//!   guitar handed to the output for monitoring.
//! - `OutputSide` (output callback): the song transport (normal or slowed-down buffer), preview clips, the guitar
//!   monitor (same callback in duplex mode, or through a drift-corrected cushion from another device), a soft
//!   limiter, and the song clock.
//!
//! Rules: no allocation, no locks, no I/O, no logging. Commands come in through lock-free queues; buffers that are
//! replaced leave through a "trash" queue so they're freed elsewhere. State for other threads is published in
//! `Shared` (atomics, a seqlock for the clock, the capture ring).

pub mod input;
pub mod onset;
pub mod output;
pub mod ring;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::clock::{AtomicF32, ClockCell};
use crate::decode::SongBuffer;
pub use onset::Onset;
pub use ring::CaptureRing;

/// One device doing input and output in the same callback (ASIO duplex): the guitar goes from input to output
/// with no buffering in between.
pub struct Duplex {
    pub input: input::InputSide,
    pub output: output::OutputSide,
}

impl Duplex {
    /// `inp`/`out` interleaved with `in_ch`/`out_ch` channels; the first input frame was captured at `capture_ns`,
    /// the first output frame will be heard at `play_ns`.
    pub fn process(
        &mut self,
        inp: &[f32],
        in_ch: usize,
        out: &mut [f32],
        out_ch: usize,
        capture_ns: u64,
        play_ns: u64,
    ) {
        let (in_ch, out_ch) = (in_ch.max(1), out_ch.max(1));
        let frames = (inp.len() / in_ch).min(out.len() / out_ch);
        let max = self.input.max_block().min(self.output.max_block());
        let rate = self.output.rate as f64;
        let mut f = 0;
        while f < frames {
            let n = (frames - f).min(max);
            let dt = (f as f64 / rate * 1e9) as u64;
            let g = self
                .input
                .process_block(&inp[f * in_ch..(f + n) * in_ch], in_ch, capture_ns + dt);
            self.output.process_block(
                &mut out[f * out_ch..(f + n) * out_ch],
                out_ch,
                play_ns + dt,
                Some(g),
            );
            f += n;
        }
        out[frames * out_ch..].fill(0.0);
    }
}

/// Input channels metered individually (the rest are ignored).
pub const MAX_IN_CHANNELS: usize = 16;

/// State published by the audio threads for everyone else.
pub struct Shared {
    pub clock: ClockCell,
    pub capture: CaptureRing,
    pub in_peak: AtomicF32,
    pub in_level: AtomicF32,
    pub ch_peaks: [AtomicF32; MAX_IN_CHANNELS],
    pub in_channels: AtomicU32,
    pub out_peak: AtomicF32,
    /// Frames into the playing buffer.
    pub frame: AtomicU64,
    pub playing: AtomicBool,
    pub finished: AtomicBool,
    /// Playing, but the slowed-down audio for this spot isn't ready yet.
    pub buffering: AtomicBool,
    pub previewing: AtomicBool,
    /// Frames into the preview clip.
    pub preview_pos: AtomicU64,
    pub preview_len: AtomicU64,
    /// The id of the clip playing (0 = none).
    pub preview_id: AtomicU64,
    /// The newest clip id the output side has taken (or that was lost with an old output).
    pub preview_started: AtomicU64,
    /// Blocks the device reported as dropped / glitched.
    pub dropouts: AtomicU32,
    /// Times the monitor cushion ran dry (two-device mode).
    pub mon_gaps: AtomicU32,
    /// Callback counts and the last callback times: a stream that stops calling back has died.
    pub in_callbacks: AtomicU64,
    pub out_callbacks: AtomicU64,
    pub last_in_ns: AtomicU64,
    pub last_out_ns: AtomicU64,
    /// Worst callback work time (ns) since the last reset, for the bench and the status page.
    pub max_work_ns: AtomicU64,
}

impl Shared {
    pub fn new(capture_bits: u32) -> Shared {
        Shared {
            clock: ClockCell::default(),
            capture: CaptureRing::new(capture_bits),
            in_peak: AtomicF32::default(),
            in_level: AtomicF32::default(),
            ch_peaks: Default::default(),
            in_channels: AtomicU32::new(0),
            out_peak: AtomicF32::default(),
            frame: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            buffering: AtomicBool::new(false),
            previewing: AtomicBool::new(false),
            preview_pos: AtomicU64::new(0),
            preview_len: AtomicU64::new(0),
            preview_id: AtomicU64::new(0),
            preview_started: AtomicU64::new(0),
            dropouts: AtomicU32::new(0),
            mon_gaps: AtomicU32::new(0),
            in_callbacks: AtomicU64::new(0),
            out_callbacks: AtomicU64::new(0),
            last_in_ns: AtomicU64::new(0),
            last_out_ns: AtomicU64::new(0),
            max_work_ns: AtomicU64::new(0),
        }
    }

    pub(crate) fn note_work(&self, ns: u64) {
        self.max_work_ns.fetch_max(ns, Ordering::Relaxed);
    }
}

/// A short stereo clip at the engine rate (a preview, a synth render).
#[derive(Debug, Default)]
pub struct Clip {
    pub rate: u32,
    /// Interleaved stereo.
    pub samples: Vec<f32>,
}

impl Clip {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }
}

/// The song slowed down (pitch kept), filled in pieces by worker threads while the audio thread plays the pieces
/// that are ready. Samples are atomics so writing and reading other pieces at the same time is data-race free.
pub struct StretchBuffer {
    pub rate: u32,
    /// Song seconds per stretched second (0.5 = half speed).
    pub speed: f64,
    pub frames: usize,
    pub chunk_frames: usize,
    samples: Box<[AtomicU32]>,
    ready: Box<[AtomicBool]>,
}

impl StretchBuffer {
    pub fn new(rate: u32, speed: f64, frames: usize, chunk_frames: usize) -> StretchBuffer {
        let chunks = frames.div_ceil(chunk_frames.max(1));
        StretchBuffer {
            rate,
            speed,
            frames,
            chunk_frames: chunk_frames.max(1),
            samples: (0..frames * 2).map(|_| AtomicU32::new(0)).collect(),
            ready: (0..chunks).map(|_| AtomicBool::new(false)).collect(),
        }
    }

    pub fn chunks(&self) -> usize {
        self.ready.len()
    }

    /// Worker side: store interleaved stereo frames starting at `frame` and mark the chunks it completes ready
    /// (`complete` lists which chunk indices are now fully written).
    pub fn write(&self, frame: usize, stereo: &[f32]) {
        for (i, s) in stereo.iter().enumerate() {
            if let Some(slot) = self.samples.get(frame * 2 + i) {
                slot.store(s.to_bits(), Ordering::Relaxed);
            }
        }
    }

    pub fn mark_ready(&self, chunk: usize) {
        if let Some(r) = self.ready.get(chunk) {
            r.store(true, Ordering::Release);
        }
    }

    pub fn chunk_ready(&self, chunk: usize) -> bool {
        self.ready
            .get(chunk)
            .map(|r| r.load(Ordering::Acquire))
            .unwrap_or(true)
    }

    /// Whether frames [a, b) can be played.
    pub fn is_ready(&self, a: usize, b: usize) -> bool {
        let b = b.min(self.frames);
        if a >= b {
            return true;
        }
        (a / self.chunk_frames..=(b - 1) / self.chunk_frames).all(|c| self.chunk_ready(c))
    }

    pub fn progress(&self) -> f64 {
        let n = self.ready.iter().filter(|r| r.load(Ordering::Relaxed)).count();
        n as f64 / self.ready.len().max(1) as f64
    }

    #[inline]
    pub fn sample(&self, i: usize) -> f32 {
        f32::from_bits(self.samples[i].load(Ordering::Relaxed))
    }
}

/// Something that shapes the guitar sound (the amp sim). `None` is "as is".
pub trait ToneProcessor: Send {
    /// Process mono samples in place.
    fn process(&mut self, x: &mut [f32]);
    fn reset(&mut self);
    /// New knob values on the running processor (no allocation).
    fn set_params(&mut self, _p: &crate::tone::ToneParams) {}
}

/// Commands for the output side.
pub enum OutCmd {
    /// Load (Some) or unload (None) the song; resets the transport to the start, paused.
    Song(Option<Arc<SongBuffer>>),
    /// Start playing, optionally from a song time (seconds).
    Play(Option<f64>),
    Pause,
    Seek(f64),
    /// Play at `speed` of the original tempo from this stretched buffer (None = the original), keeping the position.
    Speed(f64, Option<Arc<StretchBuffer>>),
    SongGain(f32),
    GuitarGain(f32),
    Monitor(bool),
    Preview {
        clip: Arc<Clip>,
        fade: bool,
        looped: bool,
        id: u64,
    },
    StopPreview {
        now: bool,
    },
    /// The guitar from another device (two-device mode): its cushion, or None when that input went away.
    Cushion(Option<Box<output::Cushion>>),
}

/// Commands for the input side.
pub enum InCmd {
    /// Which input channel is the guitar (-1 = mix all).
    Channel(i32),
    Tone(Option<Box<dyn ToneProcessor>>),
    /// New knob values for the running tone.
    ToneParams(crate::tone::ToneParams),
    Monitor(bool),
    /// Gain (linear) on the monitored guitar before the tone.
    InputGain(f32),
    /// Minimum onset energy (raised for microphones).
    OnsetFloor(f32),
}

/// Buffers on their way out of an audio thread, to be freed by the control thread.
pub enum Trash {
    Song(Arc<SongBuffer>),
    Stretch(Arc<StretchBuffer>),
    Clip(Arc<Clip>),
    Tone(Box<dyn ToneProcessor>),
    Cushion(Box<output::Cushion>),
}

/// Hand something to the trash queue without ever freeing it here (if the queue is full, leak it).
pub(crate) fn discard(trash: &mut rtrb::Producer<Trash>, t: Trash) {
    if let Err(rtrb::PushError::Full(t)) = trash.push(t) {
        std::mem::forget(t);
    }
}

/// A gain that glides to its target over a block (no zipper noise).
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    pub value: f32,
    pub target: f32,
}

impl Ramp {
    pub fn new(v: f32) -> Ramp {
        Ramp { value: v, target: v }
    }
    /// (start, step) for a block of n frames, and move on to the target.
    pub fn block(&mut self, n: usize) -> (f32, f32) {
        let start = self.value;
        let step = if n > 0 {
            (self.target - start) / n as f32
        } else {
            0.0
        };
        self.value = self.target;
        (start, step)
    }
}

/// A streaming linear-interpolation resampler for the monitor path between two device rates (RT-safe).
pub struct LinearResampler {
    /// Input samples per output sample.
    step: f64,
    pos: f64,
    last: f32,
}

impl LinearResampler {
    pub fn new(from: u32, to: u32) -> LinearResampler {
        LinearResampler {
            step: from as f64 / to as f64,
            pos: 0.0,
            last: 0.0,
        }
    }

    pub fn is_identity(&self) -> bool {
        (self.step - 1.0).abs() < 1e-12
    }

    /// Resample `x` (continuing the previous call) into `out`; returns the number of samples written.
    pub fn process(&mut self, x: &[f32], out: &mut [f32]) -> usize {
        let mut n = 0;
        // positions are relative to x, with x[-1] = self.last
        while n < out.len() {
            let i = self.pos.floor();
            let frac = (self.pos - i) as f32;
            let i = i as isize;
            if i + 1 >= x.len() as isize {
                break;
            }
            let a = if i < 0 { self.last } else { x[i as usize] };
            let b = x[(i + 1) as usize];
            out[n] = a + (b - a) * frac;
            n += 1;
            self.pos += self.step;
        }
        if let Some(&l) = x.last() {
            self.last = l;
        }
        self.pos -= x.len() as f64;
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stretch_buffer_readiness() {
        let b = StretchBuffer::new(48000, 0.5, 1000, 300);
        assert_eq!(b.chunks(), 4);
        assert!(!b.is_ready(0, 10));
        b.write(0, &[0.5; 600]);
        b.mark_ready(0);
        assert!(b.is_ready(0, 300));
        assert!(!b.is_ready(250, 350));
        assert_eq!(b.sample(1), 0.5);
        assert!(b.is_ready(1000, 1200), "past the end is ready (silence)");
    }

    #[test]
    fn linear_resampler_keeps_rate() {
        let mut r = LinearResampler::new(44100, 48000);
        let x: Vec<f32> = (0..44100)
            .map(|i| (i as f32 / 44100.0 * 440.0 * std::f32::consts::TAU).sin())
            .collect();
        let mut out = vec![0.0; 60000];
        let mut total = 0;
        for chunk in x.chunks(441) {
            let n = r.process(chunk, &mut out[total..]);
            total += n;
        }
        assert!((total as i64 - 48000).abs() <= 2, "{total}");
    }

    #[test]
    fn ramp_glides() {
        let mut g = Ramp::new(0.0);
        g.target = 1.0;
        let (s, step) = g.block(10);
        assert_eq!(s, 0.0);
        assert!((step - 0.1).abs() < 1e-6);
        assert_eq!(g.value, 1.0);
    }
}
