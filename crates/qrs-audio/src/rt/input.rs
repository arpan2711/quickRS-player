//! The input callback's work: pick the guitar channel, meter every channel, record the guitar into the capture
//! ring, find plucks, and shape the sound for monitoring (the tone), handing it to the output side.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::onset::{Onset, OnsetDetector};
use super::{InCmd, LinearResampler, MAX_IN_CHANNELS, Shared, ToneProcessor, Trash, discard};
use crate::clock::now_ns;

/// Where the monitored guitar goes.
pub enum MonitorSink {
    /// Same callback as the output (duplex): returned from `process`.
    Duplex,
    /// Another device: through this queue (the output's cushion), resampled when the rates differ.
    Queue {
        tx: rtrb::Producer<f32>,
        resampler: LinearResampler,
    },
    None,
}

pub struct InputSide {
    pub rate: u32,
    shared: Arc<Shared>,
    commands: rtrb::Consumer<InCmd>,
    trash: rtrb::Producer<Trash>,
    onsets: rtrb::Producer<Onset>,
    sink: MonitorSink,
    channel: i32,
    monitor: bool,
    input_gain: f32,
    tone: Option<Box<dyn ToneProcessor>>,
    detector: OnsetDetector,
    /// Device latency to add on top of the block timing the backend reports (seconds).
    pub extra_latency: f64,
    mono: Vec<f32>,
    /// The DC blocker: its coefficient and the last input and output (a one-pole high-pass at 5 Hz, below a
    /// bass's low B at 31 Hz: an interface's offset doesn't skew the meters, the pluck threshold or the monitor).
    dc_r: f32,
    dc_x1: f32,
    dc_y1: f32,
    shaped: Vec<f32>,
    resampled: Vec<f32>,
    peaks: [f32; MAX_IN_CHANNELS],
    level: f32,
    peak: f32,
}

pub struct InputConfig {
    pub rate: u32,
    pub channel: i32,
    pub monitor: bool,
    pub input_gain: f32,
    pub onset_floor: f32,
    pub max_block: usize,
}

impl InputSide {
    pub fn new(
        cfg: InputConfig,
        shared: Arc<Shared>,
        commands: rtrb::Consumer<InCmd>,
        trash: rtrb::Producer<Trash>,
        onsets: rtrb::Producer<Onset>,
        sink: MonitorSink,
        tone: Option<Box<dyn ToneProcessor>>,
    ) -> InputSide {
        let mut detector = OnsetDetector::new(cfg.rate);
        detector.min_energy = cfg.onset_floor;
        shared.capture.set_rate(cfg.rate);
        let cap = cfg.max_block.max(64);
        InputSide {
            rate: cfg.rate,
            shared,
            commands,
            trash,
            onsets,
            sink,
            channel: cfg.channel,
            monitor: cfg.monitor,
            input_gain: cfg.input_gain,
            tone,
            detector,
            extra_latency: 0.0,
            mono: vec![0.0; cap],
            dc_r: 1.0 - 2.0 * std::f32::consts::PI * 5.0 / cfg.rate.max(1) as f32,
            dc_x1: 0.0,
            dc_y1: 0.0,
            shaped: vec![0.0; cap],
            resampled: vec![0.0; cap * 2 + 16],
            peaks: [0.0; MAX_IN_CHANNELS],
            level: 0.0,
            peak: 0.0,
        }
    }

    fn apply_commands(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                InCmd::Channel(c) => self.channel = c,
                InCmd::Tone(t) => {
                    if let Some(old) = std::mem::replace(&mut self.tone, t) {
                        discard(&mut self.trash, Trash::Tone(old));
                    }
                }
                InCmd::ToneParams(p) => {
                    if let Some(t) = self.tone.as_mut() {
                        t.set_params(&p);
                    }
                }
                InCmd::Monitor(m) => self.monitor = m,
                InCmd::InputGain(g) => self.input_gain = g,
                InCmd::OnsetFloor(f) => self.detector.min_energy = f,
            }
        }
    }

    /// The device reported lost or glitched input.
    pub fn note_dropout(&self) {
        self.shared.dropouts.fetch_add(1, Ordering::Relaxed);
    }

    /// Longest block handled in one go (longer ones are split).
    pub fn max_block(&self) -> usize {
        self.mono.len()
    }

    /// A block of interleaved input (`channels` per frame) whose first frame was captured at `capture_ns`, of any
    /// length (split into pieces of at most `max_block` frames).
    pub fn process(&mut self, data: &[f32], channels: usize, capture_ns: u64) {
        let channels = channels.max(1);
        let frames = data.len() / channels;
        let max = self.max_block();
        let mut f = 0;
        while f < frames {
            let n = (frames - f).min(max);
            let t = capture_ns + (f as f64 / self.rate as f64 * 1e9) as u64;
            self.process_block(&data[f * channels..(f + n) * channels], channels, t);
            f += n;
        }
    }

    /// One block of at most `max_block` frames. Returns the monitored guitar (mono, toned) for duplex mode, empty
    /// otherwise.
    pub fn process_block(&mut self, data: &[f32], channels: usize, capture_ns: u64) -> &[f32] {
        let t0 = now_ns();
        self.apply_commands();
        let channels = channels.max(1);
        let frames = (data.len() / channels).min(self.mono.len());
        let sh = &self.shared;
        sh.in_callbacks.fetch_add(1, Ordering::Relaxed);
        sh.last_in_ns.store(t0, Ordering::Relaxed);
        sh.in_channels.store(channels as u32, Ordering::Relaxed);

        // meters per channel, and the guitar channel (or all of them mixed)
        let nmeter = channels.min(MAX_IN_CHANNELS);
        let mut blk_peaks = [0.0f32; MAX_IN_CHANNELS];
        let pick = if self.channel >= 0 && (self.channel as usize) < channels {
            Some(self.channel as usize)
        } else {
            None
        };
        for f in 0..frames {
            let frame = &data[f * channels..f * channels + channels];
            for (c, s) in frame.iter().take(nmeter).enumerate() {
                blk_peaks[c] = blk_peaks[c].max(s.abs());
            }
            self.mono[f] = match pick {
                Some(c) => frame[c],
                None => frame.iter().sum::<f32>() / channels as f32,
            };
        }
        for c in 0..nmeter {
            self.peaks[c] = (self.peaks[c] * 0.95).max(blk_peaks[c]);
            sh.ch_peaks[c].set(self.peaks[c]);
        }
        let (r, mut x1, mut y1) = (self.dc_r, self.dc_x1, self.dc_y1);
        for s in &mut self.mono[..frames] {
            let y = *s - x1 + r * y1;
            x1 = *s;
            // (flush denormals as the output decays to nothing)
            y1 = if y.abs() < 1e-20 { 0.0 } else { y };
            *s = y1;
        }
        self.dc_x1 = x1;
        self.dc_y1 = y1;
        let x = &self.mono[..frames];

        // record, meter, find plucks
        let start_idx = sh.capture.written();
        sh.capture.write(x);
        let (mut sum, mut pk) = (0.0f32, 0.0f32);
        for s in x {
            sum += s * s;
            pk = pk.max(s.abs());
        }
        let rms = if frames > 0 {
            (sum / frames as f32).sqrt()
        } else {
            0.0
        };
        self.level = self.level * 0.8 + rms * 0.2;
        self.peak = (self.peak * 0.95).max(pk);
        sh.in_level.set(self.level);
        sh.in_peak.set(self.peak);

        let rate = self.rate as f64;
        let base_ns = capture_ns as f64 - self.extra_latency * 1e9;
        let clock = sh.clock.load();
        let onsets = &mut self.onsets;
        self.detector.feed(x, start_idx, |idx, energy| {
            let wall = base_ns + (idx as f64 - start_idx as f64) / rate * 1e9;
            let wall = wall.max(0.0) as u64;
            let song_time = (clock.playing && clock.rate > 0.0).then(|| clock.at(wall));
            let _ = onsets.push(Onset {
                index: idx,
                wall_ns: wall,
                song_time,
                energy,
            });
        });

        // the guitar to hear
        let mut out_len = 0;
        if self.monitor {
            let g = self.input_gain;
            for (o, s) in self.shaped[..frames].iter_mut().zip(x) {
                *o = s * g;
            }
            if let Some(t) = self.tone.as_mut() {
                t.process(&mut self.shaped[..frames]);
            }
            match &mut self.sink {
                MonitorSink::Duplex => out_len = frames,
                MonitorSink::Queue { tx, resampler } => {
                    let src: &[f32] = if resampler.is_identity() {
                        &self.shaped[..frames]
                    } else {
                        let n = resampler.process(&self.shaped[..frames], &mut self.resampled);
                        &self.resampled[..n]
                    };
                    // a full queue (the output stalled) drops the newest guitar; the cushion catches up
                    if let Ok(chunk) = tx.write_chunk_uninit(src.len().min(tx.slots())) {
                        chunk.fill_from_iter(src.iter().copied());
                    }
                }
                MonitorSink::None => {}
            }
        }
        sh.note_work(now_ns().saturating_sub(t0));
        &self.shaped[..out_len]
    }
}

#[cfg(test)]
mod dc_tests {
    #[test]
    fn the_dc_blocker_removes_an_offset_and_keeps_a_low_b() {
        let rate = 48000.0f32;
        let r = 1.0 - 2.0 * std::f32::consts::PI * 5.0 / rate;
        let (mut x1, mut y1) = (0.0f32, 0.0f32);
        let mut out = Vec::new();
        for n in 0..96000 {
            let x = 0.2 + 0.5 * (2.0 * std::f32::consts::PI * 30.87 * n as f32 / rate).sin();
            let y = x - x1 + r * y1;
            x1 = x;
            y1 = y;
            out.push(y);
        }
        let tail = &out[48000..];
        let mean = tail.iter().sum::<f32>() / tail.len() as f32;
        let peak = tail.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(mean.abs() < 0.01, "offset left: {mean}");
        assert!(peak > 0.48, "the low B kept: {peak}");
    }
}
