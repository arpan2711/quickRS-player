//! The output callback's work: the song transport, preview clips, the monitored guitar, a soft limiter, the song
//! clock. Times given to commands are song seconds at the original tempo; the playing buffer is either the song
//! itself or a slowed-down copy whose frame f plays song time f / rate * speed.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{Clip, OutCmd, Ramp, Shared, StretchBuffer, Trash, discard};
use crate::clock::{SongClock, now_ns};
use crate::decode::SongBuffer;

/// Where the monitored guitar comes from.
pub enum MonitorSource {
    /// Handed to `process` by the duplex callback.
    Duplex,
    /// From another device's input callback, through a cushion kept at a steady fill.
    Cushion(Cushion),
    None,
}

/// The guitar's cushion between two devices' clocks: kept at a steady fill; when the clocks drift apart, one
/// sample more or less is taken per block (and stretched to the block, inaudibly) instead of leaving gaps. After a
/// stall it skips ahead; when it runs dry it goes quiet for a block and refills.
pub struct Cushion {
    pub rx: rtrb::Consumer<f32>,
    pub target: usize,
    primed: bool,
    fill: f64,
    scratch: Vec<f32>,
}

impl Cushion {
    pub fn new(rx: rtrb::Consumer<f32>, target: usize, max_block: usize) -> Cushion {
        Cushion {
            rx,
            target,
            primed: false,
            fill: 0.0,
            scratch: vec![0.0; max_block + 8],
        }
    }

    /// Fill `out` (mono, `frames` samples); false when there's nothing to play this block.
    fn pull(&mut self, out: &mut [f32], gaps: &std::sync::atomic::AtomicU32) -> bool {
        let frames = out.len();
        let target = self.target;
        let mut avail = self.rx.slots();
        if avail > target + 8 * frames {
            // long stall on the output side: skip ahead to the target fill
            if let Ok(c) = self.rx.read_chunk(avail - target) {
                c.commit_all();
            }
            avail = target;
        }
        if !self.primed {
            if avail < target {
                return false; // still filling
            }
            self.primed = true;
            self.fill = avail as f64;
        }
        if avail < frames {
            self.primed = false;
            gaps.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.fill = self.fill * 0.995 + avail as f64 * 0.005;
        let mut take = frames;
        if self.fill > (target + 48) as f64 && avail > frames {
            take = frames + 1;
        } else if self.fill < target as f64 - 48.0 && frames > 1 {
            take = frames - 1;
        }
        let take = take.min(self.scratch.len());
        let Ok(chunk) = self.rx.read_chunk(take) else {
            return false;
        };
        let (a, b) = chunk.as_slices();
        self.scratch[..a.len()].copy_from_slice(a);
        self.scratch[a.len()..a.len() + b.len()].copy_from_slice(b);
        chunk.commit_all();
        if take == frames {
            out.copy_from_slice(&self.scratch[..frames]);
        } else {
            let scale = (take - 1) as f32 / (frames - 1).max(1) as f32;
            for (i, o) in out.iter_mut().enumerate() {
                let p = i as f32 * scale;
                let k = (p as usize).min(take - 1);
                let frac = p - k as f32;
                let b = self.scratch[(k + 1).min(take - 1)];
                *o = self.scratch[k] + (b - self.scratch[k]) * frac;
            }
        }
        true
    }
}

struct Preview {
    clip: Arc<Clip>,
    pos: usize,
    gain: f32,
    target: f32,
    looped: bool,
    /// Seconds for a full fade in or out.
    fade: f32,
    id: u64,
}

/// Mix a clip into `mix` (stereo); true when it has finished (played out, or faded to nothing).
fn mix_clip(p: &mut Preview, mix: &mut [f32], frames: usize, rate: u32, sg: f32) -> bool {
    let clip_frames = p.clip.frames();
    let fade_step = frames as f32 / (rate as f32 * p.fade);
    let g0 = p.gain;
    let g1 = if p.target > g0 {
        (g0 + fade_step).min(p.target)
    } else {
        (g0 - fade_step).max(p.target)
    };
    let mut i = 0;
    while i < frames {
        if p.pos >= clip_frames {
            if p.looped && clip_frames > 0 {
                p.pos = 0;
            } else {
                break;
            }
        }
        let n = (frames - i).min(clip_frames - p.pos);
        for k in 0..n {
            let g = (g0 + (g1 - g0) * (i + k) as f32 / frames as f32) * sg;
            mix[2 * (i + k)] += p.clip.samples[2 * (p.pos + k)] * g;
            mix[2 * (i + k) + 1] += p.clip.samples[2 * (p.pos + k) + 1] * g;
        }
        p.pos += n;
        i += n;
    }
    p.gain = g1;
    (g1 <= 0.0 && p.target <= 0.0) || (!p.looped && p.pos >= clip_frames)
}

/// What the output side starts with (carried over when devices are reopened).
#[derive(Clone)]
pub struct OutputState {
    pub song: Option<Arc<SongBuffer>>,
    pub stretched: Option<Arc<StretchBuffer>>,
    pub speed: f64,
    /// Song seconds.
    pub position: f64,
    pub playing: bool,
    pub song_gain: f32,
    pub guitar_gain: f32,
    pub monitor: bool,
}

impl Default for OutputState {
    fn default() -> Self {
        OutputState {
            song: None,
            stretched: None,
            speed: 1.0,
            position: 0.0,
            playing: false,
            song_gain: 0.8,
            guitar_gain: 0.8,
            monitor: true,
        }
    }
}

pub struct OutputSide {
    pub rate: u32,
    shared: Arc<Shared>,
    commands: rtrb::Consumer<OutCmd>,
    trash: rtrb::Producer<Trash>,
    monitor: MonitorSource,
    song: Option<Arc<SongBuffer>>,
    stretched: Option<Arc<StretchBuffer>>,
    speed: f64,
    /// Frames into the playing buffer (the song, or the stretched copy).
    frame: usize,
    playing: bool,
    song_gain: Ramp,
    guitar_gain: Ramp,
    monitor_on: bool,
    preview: Option<Preview>,
    /// A clip that was replaced, fading out under the new one.
    preview_old: Option<Preview>,
    /// The newest preview id taken from the queue (published after the clip's state).
    preview_taken: u64,
    /// Which output channels get the stereo mix (RS_ASIO's BaseChannel).
    pub base_channel: usize,
    /// Output latency beyond what the backend's timestamps include (seconds).
    pub extra_latency: f64,
    mix: Vec<f32>,
    guitar: Vec<f32>,
}

impl OutputSide {
    pub fn new(
        rate: u32,
        state: OutputState,
        shared: Arc<Shared>,
        commands: rtrb::Consumer<OutCmd>,
        trash: rtrb::Producer<Trash>,
        monitor: MonitorSource,
        max_block: usize,
    ) -> OutputSide {
        let mut o = OutputSide {
            rate,
            shared,
            commands,
            trash,
            monitor,
            song: state.song,
            stretched: state.stretched,
            speed: state.speed,
            frame: 0,
            playing: state.playing,
            song_gain: Ramp::new(state.song_gain),
            guitar_gain: Ramp::new(state.guitar_gain),
            monitor_on: state.monitor,
            preview: None,
            preview_old: None,
            preview_taken: 0,
            base_channel: 0,
            extra_latency: 0.0,
            mix: vec![0.0; max_block.max(64) * 2],
            guitar: vec![0.0; max_block.max(64)],
        };
        o.frame = o.frame_for(state.position);
        o.publish(now_ns(), 0);
        o
    }

    fn song_len(&self) -> usize {
        match (&self.stretched, &self.song) {
            (Some(s), _) => s.frames,
            (None, Some(s)) => s.frames(),
            _ => 0,
        }
    }

    fn duration(&self) -> f64 {
        self.song.as_ref().map(|s| s.duration()).unwrap_or(0.0)
    }

    fn frame_for(&self, t: f64) -> usize {
        let t = t.clamp(0.0, self.duration());
        (t / self.speed * self.rate as f64) as usize
    }

    fn time_of(&self, frame: usize) -> f64 {
        (frame as f64 / self.rate as f64 * self.speed).min(self.duration())
    }

    fn finished(&self) -> bool {
        self.song.is_none()
            || self.frame + (self.rate as usize / 20) >= self.song_len()
            || self.time_of(self.frame) >= self.duration() - 0.05
    }

    fn apply_commands(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                OutCmd::Song(s) => {
                    if let Some(old) = std::mem::replace(&mut self.song, s) {
                        discard(&mut self.trash, Trash::Song(old));
                    }
                    if let Some(old) = self.stretched.take() {
                        discard(&mut self.trash, Trash::Stretch(old));
                    }
                    self.speed = 1.0;
                    self.frame = 0;
                    self.playing = false;
                }
                OutCmd::Play(from) => {
                    if self.song.is_some() {
                        if let Some(t) = from {
                            self.frame = self.frame_for(t);
                        } else if self.finished() {
                            self.frame = 0;
                        }
                        self.playing = true;
                    }
                }
                OutCmd::Pause => self.playing = false,
                OutCmd::Seek(t) => self.frame = self.frame_for(t),
                OutCmd::Speed(speed, buf) => {
                    let t = self.time_of(self.frame);
                    if let Some(old) = std::mem::replace(&mut self.stretched, buf) {
                        discard(&mut self.trash, Trash::Stretch(old));
                    }
                    self.speed = if self.stretched.is_some() { speed } else { 1.0 };
                    self.frame = self.frame_for(t);
                }
                OutCmd::SongGain(g) => self.song_gain.target = g,
                OutCmd::GuitarGain(g) => self.guitar_gain.target = g,
                OutCmd::Monitor(m) => self.monitor_on = m,
                OutCmd::Preview {
                    clip,
                    fade,
                    looped,
                    id,
                } => {
                    let p = Preview {
                        clip,
                        pos: 0,
                        gain: if fade { 0.0 } else { 1.0 },
                        target: 1.0,
                        looped,
                        fade: 0.25,
                        id,
                    };
                    self.preview_taken = id;
                    if let Some(mut old) = self.preview.replace(p) {
                        // the clip being replaced fades out quickly under the new one (no pop on restart)
                        old.target = 0.0;
                        old.fade = 0.02;
                        if let Some(older) = self.preview_old.replace(old) {
                            discard(&mut self.trash, Trash::Clip(older.clip));
                        }
                    }
                }
                OutCmd::Cushion(c) => {
                    let new = match c {
                        Some(c) => MonitorSource::Cushion(*c),
                        None => MonitorSource::None,
                    };
                    if let MonitorSource::Cushion(old) = std::mem::replace(&mut self.monitor, new) {
                        discard(&mut self.trash, Trash::Cushion(Box::new(old)));
                    }
                }
                OutCmd::StopPreview { now } => {
                    if now {
                        for old in [self.preview.take(), self.preview_old.take()]
                            .into_iter()
                            .flatten()
                        {
                            discard(&mut self.trash, Trash::Clip(old.clip));
                        }
                    } else if let Some(p) = self.preview.as_mut() {
                        p.target = 0.0;
                    }
                }
            }
        }
    }

    /// Mix the song into the stereo scratch; returns false while the slowed-down audio here isn't ready.
    fn mix_song(&mut self, frames: usize) -> bool {
        if !self.playing {
            return true;
        }
        let start = self.frame;
        let len = self.song_len();
        let n = frames.min(len.saturating_sub(start));
        let (g0, step) = self.song_gain.block(frames);
        if let Some(st) = &self.stretched {
            if !st.is_ready(start, start + frames) {
                return false;
            }
            for i in 0..n {
                let g = g0 + step * i as f32;
                self.mix[2 * i] += st.sample(2 * (start + i)) * g;
                self.mix[2 * i + 1] += st.sample(2 * (start + i) + 1) * g;
            }
        } else if let Some(song) = &self.song {
            let s = &song.samples[2 * start..2 * (start + n)];
            for i in 0..n {
                let g = (g0 + step * i as f32) / 32768.0;
                self.mix[2 * i] += s[2 * i] as f32 * g;
                self.mix[2 * i + 1] += s[2 * i + 1] as f32 * g;
            }
        }
        self.frame = start + frames;
        if self.frame >= len {
            self.frame = len;
            self.playing = false;
        }
        true
    }

    fn mix_preview(&mut self, frames: usize) {
        let sg = self.song_gain.value;
        if let Some(p) = self.preview_old.as_mut()
            && mix_clip(p, &mut self.mix, frames, self.rate, sg)
            && let Some(old) = self.preview_old.take()
        {
            discard(&mut self.trash, Trash::Clip(old.clip));
        }
        let Some(p) = self.preview.as_mut() else { return };
        let done = mix_clip(p, &mut self.mix, frames, self.rate, sg);
        self.shared.preview_pos.store(p.pos as u64, Ordering::Relaxed);
        self.shared
            .preview_len
            .store(p.clip.frames() as u64, Ordering::Relaxed);
        if done && let Some(old) = self.preview.take() {
            discard(&mut self.trash, Trash::Clip(old.clip));
        }
    }

    fn publish(&mut self, play_ns: u64, frames: usize) {
        let sh = &self.shared;
        let wall = (play_ns as f64 + self.extra_latency * 1e9).max(0.0) as u64;
        let buffering = sh.buffering.load(Ordering::Relaxed);
        let start_time =
            self.time_of(
                self.frame
                    .saturating_sub(if self.playing && !buffering { frames } else { 0 }),
            );
        let clock = SongClock {
            wall_ns: wall,
            song_time: if self.playing && !buffering {
                start_time
            } else {
                self.time_of(self.frame)
            },
            rate: if self.playing && !buffering {
                self.speed
            } else {
                0.0
            },
            limit: self.time_of(self.frame),
            playing: self.playing,
        };
        sh.clock.store(&clock);
        sh.frame.store(self.frame as u64, Ordering::Relaxed);
        sh.playing.store(self.playing, Ordering::Relaxed);
        sh.finished.store(self.finished(), Ordering::Relaxed);
        let live = self.preview.as_ref().filter(|p| p.target > 0.0);
        sh.previewing.store(live.is_some(), Ordering::Relaxed);
        sh.preview_id
            .store(live.map(|p| p.id).unwrap_or(0), Ordering::Release);
        sh.preview_started
            .fetch_max(self.preview_taken, Ordering::Release);
    }

    /// The device ran dry (the output wasn't refilled in time).
    pub fn note_dropout(&self) {
        self.shared.dropouts.fetch_add(1, Ordering::Relaxed);
    }

    /// Longest block handled in one go (longer ones are split).
    pub fn max_block(&self) -> usize {
        self.guitar.len()
    }

    /// A block of output of any length (split into pieces of at most `max_block` frames): `out` is interleaved
    /// with `channels` per frame; its first frame will be heard at `play_ns`.
    pub fn process(&mut self, out: &mut [f32], channels: usize, play_ns: u64) {
        let channels = channels.max(1);
        let frames = out.len() / channels;
        let max = self.max_block();
        let mut f = 0;
        while f < frames {
            let n = (frames - f).min(max);
            let t = play_ns + (f as f64 / self.rate as f64 * 1e9) as u64;
            self.process_block(&mut out[f * channels..(f + n) * channels], channels, t, None);
            f += n;
        }
    }

    /// One block of at most `max_block` frames. `duplex_guitar` is the monitored guitar from the same callback's
    /// input (duplex mode).
    pub fn process_block(
        &mut self,
        out: &mut [f32],
        channels: usize,
        play_ns: u64,
        duplex_guitar: Option<&[f32]>,
    ) {
        let t0 = now_ns();
        self.apply_commands();
        let channels = channels.max(1);
        let frames = (out.len() / channels).min(self.guitar.len());
        let sh = self.shared.clone();
        sh.out_callbacks.fetch_add(1, Ordering::Relaxed);
        sh.last_out_ns.store(t0, Ordering::Relaxed);
        self.mix[..2 * frames].fill(0.0);

        let was_playing = self.playing;
        let ready = self.mix_song(frames);
        sh.buffering.store(was_playing && !ready, Ordering::Relaxed);
        self.mix_preview(frames);

        // the guitar
        let mut have_guitar = false;
        match (&mut self.monitor, duplex_guitar) {
            (MonitorSource::Duplex, Some(g)) if !g.is_empty() => {
                let n = g.len().min(frames);
                self.guitar[..n].copy_from_slice(&g[..n]);
                self.guitar[n..frames].fill(0.0);
                have_guitar = true;
            }
            (MonitorSource::Cushion(c), _) => {
                have_guitar = c.pull(&mut self.guitar[..frames], &sh.mon_gaps) && self.monitor_on;
            }
            _ => {}
        }
        let (gg0, gstep) = self.guitar_gain.block(frames);
        if have_guitar && self.monitor_on {
            for i in 0..frames {
                let g = self.guitar[i] * (gg0 + gstep * i as f32);
                self.mix[2 * i] += g;
                self.mix[2 * i + 1] += g;
            }
        }

        // limit and write out
        let mut peak = 0.0f32;
        out.fill(0.0);
        for i in 0..frames {
            let l = soft_clip(self.mix[2 * i]);
            let r = soft_clip(self.mix[2 * i + 1]);
            peak = peak.max(l.abs()).max(r.abs());
            let f = &mut out[i * channels..i * channels + channels];
            if channels == 1 {
                f[0] = 0.5 * (l + r);
            } else {
                let b = self.base_channel.min(channels - 2);
                f[b] = l;
                f[b + 1] = r;
            }
        }
        sh.out_peak.set(peak.max(sh.out_peak.get() * 0.95));
        self.publish(play_ns, frames);
        sh.note_work(now_ns().saturating_sub(t0));
    }
}

/// Transparent below 0.85, then a smooth knee that never exceeds 1.0 (instead of hard clipping).
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    const K: f32 = 0.85;
    let a = x.abs();
    if a <= K {
        x
    } else {
        let y = K + (1.0 - K) * ((a - K) / (1.0 - K)).tanh();
        y.copysign(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_clip_is_bounded_and_transparent() {
        assert_eq!(soft_clip(0.5), 0.5);
        assert!(soft_clip(10.0) <= 1.0 && soft_clip(10.0) > 0.99);
        assert!((soft_clip(-0.9) + soft_clip(0.9)).abs() < 1e-7);
    }

    #[test]
    fn a_replaced_clip_fades_out_and_ids_are_reported() {
        use std::sync::atomic::Ordering::Relaxed;
        let shared = Arc::new(Shared::new(12));
        let (mut tx, rx) = rtrb::RingBuffer::<OutCmd>::new(16);
        let (trash, _trash_rx) = rtrb::RingBuffer::<Trash>::new(16);
        let mut o = OutputSide::new(
            48000,
            OutputState::default(),
            shared.clone(),
            rx,
            trash,
            MonitorSource::None,
            256,
        );
        let dc = |v: f32| {
            Arc::new(Clip {
                rate: 48000,
                samples: vec![v; 48000 * 2],
            })
        };
        let preview = |clip, id| OutCmd::Preview {
            clip,
            fade: false,
            looped: true,
            id,
        };
        assert!(tx.push(preview(dc(0.5), 1)).is_ok());
        let mut out = vec![0.0f32; 512];
        o.process(&mut out, 2, 0);
        assert_eq!(shared.preview_id.load(Relaxed), 1);
        assert_eq!(shared.preview_started.load(Relaxed), 1);
        // the new clip starts from silence; the old one must not stop dead under it
        assert!(tx.push(preview(dc(0.0), 2)).is_ok());
        let mut prev = out[out.len() - 2];
        let mut worst = 0.0f32;
        for _ in 0..8 {
            o.process(&mut out, 2, 0);
            for f in out.chunks(2) {
                worst = worst.max((f[0] - prev).abs());
                prev = f[0];
            }
        }
        assert!(worst < 0.002, "largest step {worst}");
        assert_eq!(prev, 0.0, "the old clip has faded out");
        assert_eq!(shared.preview_id.load(Relaxed), 2);
        assert!(tx.push(OutCmd::StopPreview { now: true }).is_ok());
        o.process(&mut out, 2, 0);
        assert_eq!(shared.preview_id.load(Relaxed), 0);
        assert_eq!(shared.preview_started.load(Relaxed), 2);
    }
}
