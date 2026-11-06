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
        /// Frames of (toned, dry) pairs.
        tx: rtrb::Producer<f32>,
        resampler: LinearResampler,
        resampler_dry: LinearResampler,
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
    /// Hand the guitar on even with monitoring off (the looper, Rewind).
    feed: bool,
    input_gain: f32,
    tone: Option<Box<dyn ToneProcessor>>,
    /// The tone being faded out after a swap (`Some(None)`: fading out from the dry guitar), and how far.
    fading: Option<Option<Box<dyn ToneProcessor>>>,
    fade_pos: usize,
    fade_len: usize,
    /// The fading tone's own copy of the block.
    faded: Vec<f32>,
    /// The toned guitar's peak (the board's output meter).
    tone_peak: f32,
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
    /// The guitar before the tone (for the looper's re-amp and Rewind).
    dry: Vec<f32>,
    resampled: Vec<f32>,
    resampled_dry: Vec<f32>,
    pairs: Vec<f32>,
    peaks: [f32; MAX_IN_CHANNELS],
    level: f32,
    peak: f32,
}

pub struct InputConfig {
    pub rate: u32,
    pub channel: i32,
    pub monitor: bool,
    pub feed: bool,
    pub input_gain: f32,
    pub onset_floor: f32,
    pub onset_bass: bool,
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
        detector.bass = cfg.onset_bass;
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
            feed: cfg.feed,
            input_gain: cfg.input_gain,
            tone,
            fading: None,
            fade_pos: 0,
            fade_len: (cfg.rate as usize * 30 / 1000).max(1),
            faded: vec![0.0; cap],
            tone_peak: 0.0,
            detector,
            extra_latency: 0.0,
            mono: vec![0.0; cap],
            dc_r: 1.0 - 2.0 * std::f32::consts::PI * 5.0 / cfg.rate.max(1) as f32,
            dc_x1: 0.0,
            dc_y1: 0.0,
            shaped: vec![0.0; cap],
            dry: vec![0.0; cap],
            resampled: vec![0.0; cap * 2 + 16],
            resampled_dry: vec![0.0; cap * 2 + 16],
            pairs: vec![0.0; cap * 4 + 32],
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
                    // crossfade from the old tone (or the dry guitar) over 30 ms; a swap during a fade drops the
                    // tone that was already on its way out
                    let old = std::mem::replace(&mut self.tone, t);
                    if let Some(Some(gone)) = self.fading.replace(old) {
                        discard(&mut self.trash, Trash::Tone(gone));
                    }
                    self.fade_pos = 0;
                }
                InCmd::Board(c) => {
                    let spare = match self.tone.as_mut() {
                        Some(t) => t.board(c),
                        None => c.into_pedal(),
                    };
                    if let Some(p) = spare {
                        discard(&mut self.trash, Trash::Pedal(p));
                    }
                }
                InCmd::ToneParams(p) => {
                    if let Some(t) = self.tone.as_mut() {
                        t.set_params(&p);
                    }
                }
                InCmd::Monitor(m) => self.monitor = m,
                InCmd::Feed(f) => self.feed = f,
                InCmd::InputGain(g) => self.input_gain = g,
                InCmd::OnsetFloor(f) => self.detector.min_energy = f,
                InCmd::OnsetBass(b) => self.detector.bass = b,
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

    /// One block of at most `max_block` frames. Returns the guitar for duplex mode (mono: toned, and dry before the
    /// tone), empty otherwise.
    pub fn process_block(&mut self, data: &[f32], channels: usize, capture_ns: u64) -> (&[f32], &[f32]) {
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
        // the interface clipping: the guitar's raw samples at full scale (before the DC blocker moves them)
        let raw = match pick {
            Some(c) if c < nmeter => blk_peaks[c],
            _ => blk_peaks[..nmeter].iter().fold(0.0f32, |m, &p| m.max(p)),
        };
        if raw >= 0.999 {
            sh.in_clips.fetch_add(1, Ordering::Relaxed);
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
        if self.monitor || self.feed {
            let g = self.input_gain;
            for ((o, d), s) in self.shaped[..frames]
                .iter_mut()
                .zip(self.dry[..frames].iter_mut())
                .zip(x)
            {
                *o = s * g;
                *d = *o;
            }
            if self.fading.is_some() {
                self.faded[..frames].copy_from_slice(&self.shaped[..frames]);
                if let Some(Some(old)) = self.fading.as_mut() {
                    old.process(&mut self.faded[..frames]);
                }
            }
            if let Some(t) = self.tone.as_mut() {
                t.process(&mut self.shaped[..frames]);
                while let Some(p) = t.trash() {
                    discard(&mut self.trash, Trash::Pedal(p));
                }
            }
            if self.fading.is_some() {
                let len = self.fade_len as f32;
                for (i, (o, f)) in self.shaped[..frames]
                    .iter_mut()
                    .zip(&self.faded[..frames])
                    .enumerate()
                {
                    let w = ((self.fade_pos + i) as f32 / len).min(1.0);
                    *o = *f + (*o - *f) * w;
                }
                self.fade_pos += frames;
                if self.fade_pos >= self.fade_len
                    && let Some(Some(gone)) = self.fading.take()
                {
                    discard(&mut self.trash, Trash::Tone(gone));
                }
                if self.fade_pos >= self.fade_len {
                    self.fading = None;
                }
            }
            let pk = self.shaped[..frames].iter().fold(0.0f32, |m, s| m.max(s.abs()));
            self.tone_peak = (self.tone_peak * 0.95).max(pk);
            sh.tone_peak.set(self.tone_peak);
            match &mut self.sink {
                MonitorSink::Duplex => out_len = frames,
                MonitorSink::Queue {
                    tx,
                    resampler,
                    resampler_dry,
                } => {
                    let (wet, dry): (&[f32], &[f32]) = if resampler.is_identity() {
                        (&self.shaped[..frames], &self.dry[..frames])
                    } else {
                        let n = resampler.process(&self.shaped[..frames], &mut self.resampled);
                        let m = resampler_dry.process(&self.dry[..frames], &mut self.resampled_dry);
                        let n = n.min(m);
                        (&self.resampled[..n], &self.resampled_dry[..n])
                    };
                    let n = wet.len().min(self.pairs.len() / 2);
                    for i in 0..n {
                        self.pairs[2 * i] = wet[i];
                        self.pairs[2 * i + 1] = dry[i];
                    }
                    // a full queue (the output stalled) drops the newest guitar; the cushion catches up
                    let room = (tx.slots() / 2).min(n) * 2;
                    if let Ok(chunk) = tx.write_chunk_uninit(room) {
                        chunk.fill_from_iter(self.pairs[..room].iter().copied());
                    }
                }
                MonitorSink::None => {}
            }
        }
        // when the newest sample handed on was played (the looper measures the round trip from it)
        let newest = capture_ns as f64 + frames as f64 / rate * 1e9 - self.extra_latency * 1e9;
        sh.in_newest_ns.store(newest.max(0.0) as u64, Ordering::Relaxed);
        sh.note_work(now_ns().saturating_sub(t0));
        (&self.shaped[..out_len], &self.dry[..out_len])
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
