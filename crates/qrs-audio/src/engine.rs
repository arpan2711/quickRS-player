//! The engine the app talks to: song transport, volumes, monitoring, the tone, preview clips, the guitar capture,
//! onsets and meters, and which devices it runs on.
//!
//! A control thread owns the device streams (some backends' streams can't move between threads). It opens a route
//! (one duplex device, or an input and an output on their own clocks), reopens a side that fails (unplugged,
//! driver reset) every 1.5 s until it comes back, and frees whatever the audio threads hand back. With no output
//! device the song plays on a silent timer, so the clock (and the chart, and scoring) keeps going.
//!
//! Everything the app sets is also kept here, so a reopened stream starts exactly where the old one was.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;

use crate::backend::{self, Backend, BufferSize, DeviceId, InputRequest, OutputRequest, Stream, StreamInfo};
use crate::clock::now_ns;
use crate::decode::{SongBuffer, resample};
use crate::rt::input::{InputConfig, InputSide, MonitorSink};
use crate::rt::onset::Onset;
use crate::rt::output::{Cushion, MonitorSource, OutputSide, OutputState};
use crate::rt::{Clip, Duplex, InCmd, LinearResampler, OutCmd, Shared, StretchBuffer, Trash};
use crate::tone::{ToneSpec, make_tone};

/// Where the engine runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub input: Option<InputRoute>,
    /// None: no sound card (the song plays on a silent timer).
    pub output: Option<DeviceId>,
    /// WASAPI exclusive mode where possible (lowest latency; other apps go quiet).
    pub exclusive: bool,
    pub buffer: BufferSize,
    /// Output channels for the stereo mix (RS_ASIO's BaseChannel).
    pub base_channel: usize,
}

impl Default for Route {
    fn default() -> Self {
        Route {
            input: None,
            output: None,
            exclusive: false,
            buffer: BufferSize::Default,
            base_channel: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputRoute {
    pub device: DeviceId,
    /// Guitar channel (0-based; -1 mixes all).
    pub channel: i32,
}

/// What the engine is running on right now.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub route: Route,
    pub input: Option<StreamInfo>,
    pub output: Option<StreamInfo>,
    pub duplex: bool,
    /// Why the input / output isn't running (couldn't open, unplugged and retrying).
    pub input_error: Option<String>,
    pub output_error: Option<String>,
    /// The output is the silent timer (no device, or the device is gone).
    pub silent_output: bool,
}

/// Something the app may want to tell the user about.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    InputLost(String),
    InputBack(String),
    OutputLost(String),
    OutputBack(String),
    /// The route changed (opened, or a side failed over).
    Changed,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Meters {
    /// Guitar channel peak and level (RMS), 0..1.
    pub peak: f32,
    pub level: f32,
    /// Peak of every input channel (for picking the guitar channel).
    pub channels: [f32; crate::rt::MAX_IN_CHANNELS],
    pub channel_count: usize,
    pub output_peak: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub dropouts: u32,
    pub monitor_gaps: u32,
    pub max_work_ns: u64,
    pub in_callbacks: u64,
    pub out_callbacks: u64,
}

/// What the app set, kept to rebuild a reopened stream.
#[derive(Clone)]
struct Persist {
    song: Option<Arc<SongBuffer>>,
    stretched: Option<Arc<StretchBuffer>>,
    speed: f64,
    song_gain: f32,
    guitar_gain: f32,
    monitor: bool,
    channel: i32,
    input_gain: f32,
    onset_floor: f32,
    tone: ToneSpec,
}

impl Default for Persist {
    fn default() -> Self {
        Persist {
            song: None,
            stretched: None,
            speed: 1.0,
            song_gain: 0.8,
            guitar_gain: 0.8,
            monitor: true,
            channel: 0,
            input_gain: 1.0,
            onset_floor: 0.004,
            tone: ToneSpec::AsIs,
        }
    }
}

struct Ctl {
    out_tx: Option<rtrb::Producer<OutCmd>>,
    in_tx: Option<rtrb::Producer<InCmd>>,
    onsets_rx: Option<rtrb::Consumer<Onset>>,
    persist: Persist,
    status: Status,
    /// Output rate the RT side runs at (song buffers must match).
    rate: u32,
    /// The id of the newest preview clip sent.
    preview_issued: u64,
}

impl Ctl {
    /// The output's command queue is being replaced: previews sent to the old one will never start.
    fn set_out_tx(&mut self, shared: &Shared, tx: Option<rtrb::Producer<OutCmd>>) {
        shared
            .preview_started
            .fetch_max(self.preview_issued, Ordering::Relaxed);
        self.out_tx = tx;
    }
}

struct Inner {
    shared: Arc<Shared>,
    ctl: Mutex<Ctl>,
    tx: Sender<Msg>,
    events: Receiver<Event>,
    last_pos: Mutex<f64>,
}

enum Msg {
    Open(Route),
    Shutdown,
}

/// Where a preview clip is (see `Engine::preview_state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewState {
    /// Sent, not playing yet.
    Pending,
    Playing,
    /// Finished, stopped or replaced.
    Over,
}

pub struct Engine {
    inner: Arc<Inner>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Start the engine with no devices (the silent timer); call `open` with a route.
    pub fn new() -> Engine {
        let shared = Arc::new(Shared::new(18));
        let (tx, rx) = unbounded();
        let (ev_tx, ev_rx) = unbounded();
        let inner = Arc::new(Inner {
            shared,
            ctl: Mutex::new(Ctl {
                out_tx: None,
                in_tx: None,
                onsets_rx: None,
                persist: Persist::default(),
                status: Status::default(),
                rate: 48000,
                preview_issued: 0,
            }),
            tx,
            events: ev_rx,
            last_pos: Mutex::new(0.0),
        });
        let worker = inner.clone();
        let thread = std::thread::Builder::new()
            .name("qrs-audio-control".into())
            .spawn(move || control_loop(worker, rx, ev_tx))
            .expect("spawn audio control thread");
        let e = Engine {
            inner,
            thread: Some(thread),
        };
        e.open(Route::default());
        e
    }

    /// Run on these devices (closing whatever ran before), listening to the route's guitar channel.
    pub fn open(&self, route: Route) {
        if let Some(i) = &route.input {
            self.inner.ctl.lock().persist.channel = i.channel;
        }
        let _ = self.inner.tx.send(Msg::Open(route));
    }

    /// Wait until the control thread has applied everything sent so far (tests, the CLI).
    pub fn settle(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if self.inner.tx.is_empty() && self.inner.ctl.lock().out_tx.is_some() {
                std::thread::sleep(Duration::from_millis(20));
                if self.inner.tx.is_empty() {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    pub fn status(&self) -> Status {
        self.inner.ctl.lock().status.clone()
    }

    /// The rate songs and clips play at (the output stream's).
    pub fn rate(&self) -> u32 {
        self.inner.ctl.lock().rate
    }

    pub fn events(&self) -> Vec<Event> {
        self.inner.events.try_iter().collect()
    }

    fn out(&self, cmd: OutCmd) {
        let mut c = self.inner.ctl.lock();
        if let Some(tx) = c.out_tx.as_mut() {
            let _ = tx.push(cmd);
        }
    }

    fn inp(&self, cmd: InCmd) {
        let mut c = self.inner.ctl.lock();
        if let Some(tx) = c.in_tx.as_mut() {
            let _ = tx.push(cmd);
        }
    }

    // -- song transport ----------------------------------------------------------------------------------
    /// Load a song (resampled to the engine rate if needed). Paused at the start.
    pub fn load_song(&self, song: SongBuffer) {
        let rate = self.rate();
        let song = if song.rate == rate {
            song
        } else {
            resample_song(&song, rate)
        };
        let song = Arc::new(song);
        {
            let mut c = self.inner.ctl.lock();
            c.persist.song = Some(song.clone());
            c.persist.stretched = None;
            c.persist.speed = 1.0;
        }
        *self.inner.last_pos.lock() = 0.0;
        self.out(OutCmd::Song(Some(song)));
    }

    pub fn unload(&self) {
        {
            let mut c = self.inner.ctl.lock();
            c.persist.song = None;
            c.persist.stretched = None;
            c.persist.speed = 1.0;
        }
        self.out(OutCmd::Song(None));
    }

    pub fn duration(&self) -> f64 {
        self.inner
            .ctl
            .lock()
            .persist
            .song
            .as_ref()
            .map(|s| s.duration())
            .unwrap_or(0.0)
    }

    pub fn has_song(&self) -> bool {
        self.inner.ctl.lock().persist.song.is_some()
    }

    pub fn play(&self, from: Option<f64>) {
        if let Some(t) = from {
            *self.inner.last_pos.lock() = t;
        } else if self.finished() {
            *self.inner.last_pos.lock() = 0.0;
        }
        self.out(OutCmd::Play(from));
        self.wait_applied(|s| s.playing.load(Ordering::Relaxed));
    }

    pub fn pause(&self) {
        self.out(OutCmd::Pause);
        self.wait_applied(|s| !s.playing.load(Ordering::Relaxed));
    }

    pub fn toggle(&self) {
        if self.playing() {
            self.pause()
        } else {
            self.play(if self.finished() { Some(0.0) } else { None })
        }
    }

    pub fn seek(&self, t: f64) {
        *self.inner.last_pos.lock() = t.max(0.0);
        self.out(OutCmd::Seek(t.max(0.0)));
    }

    /// Commands reach the audio thread within a block; transport calls wait up to ~50 ms for it so the next
    /// position read is consistent.
    fn wait_applied(&self, done: impl Fn(&Shared) -> bool) {
        let start = Instant::now();
        while !done(&self.inner.shared) && start.elapsed() < Duration::from_millis(50) {
            std::thread::sleep(Duration::from_micros(500));
        }
    }

    pub fn playing(&self) -> bool {
        self.inner.shared.playing.load(Ordering::Relaxed)
    }

    pub fn finished(&self) -> bool {
        self.inner.shared.finished.load(Ordering::Relaxed)
    }

    pub fn buffering(&self) -> bool {
        self.inner.shared.buffering.load(Ordering::Relaxed)
    }

    /// The song time being heard now (seconds, original tempo). Never steps backwards while playing.
    pub fn position(&self) -> f64 {
        let c = self.inner.shared.clock.load();
        let t = c.at(now_ns());
        let mut last = self.inner.last_pos.lock();
        if c.playing && c.rate > 0.0 {
            let t = t.max(*last);
            *last = t;
            t
        } else {
            *last = t;
            t
        }
    }

    /// The song clock (for converting wall times to song times elsewhere).
    pub fn clock(&self) -> crate::clock::SongClock {
        self.inner.shared.clock.load()
    }

    /// Play at `speed` of the original tempo from a slowed-down buffer (None: back to the original).
    pub fn set_speed(&self, speed: f64, stretched: Option<Arc<StretchBuffer>>) {
        {
            let mut c = self.inner.ctl.lock();
            c.persist.speed = if stretched.is_some() { speed } else { 1.0 };
            c.persist.stretched = stretched.clone();
        }
        self.out(OutCmd::Speed(speed, stretched));
    }

    pub fn speed(&self) -> f64 {
        self.inner.ctl.lock().persist.speed
    }

    // -- levels and the guitar -----------------------------------------------------------------------------
    pub fn set_song_gain(&self, g: f32) {
        self.inner.ctl.lock().persist.song_gain = g;
        self.out(OutCmd::SongGain(g));
    }

    pub fn song_gain(&self) -> f32 {
        self.inner.ctl.lock().persist.song_gain
    }

    pub fn set_guitar_gain(&self, g: f32) {
        self.inner.ctl.lock().persist.guitar_gain = g;
        self.out(OutCmd::GuitarGain(g));
    }

    pub fn guitar_gain(&self) -> f32 {
        self.inner.ctl.lock().persist.guitar_gain
    }

    /// Hear the guitar through the output (the input is listened to either way).
    pub fn set_monitor(&self, on: bool) {
        self.inner.ctl.lock().persist.monitor = on;
        self.out(OutCmd::Monitor(on));
        self.inp(InCmd::Monitor(on));
    }

    pub fn monitor(&self) -> bool {
        self.inner.ctl.lock().persist.monitor
    }

    /// The guitar channel (0-based; -1 mixes all), without reopening the input.
    pub fn set_channel(&self, ch: i32) {
        {
            let mut c = self.inner.ctl.lock();
            c.persist.channel = ch;
            if let Some(i) = c.status.route.input.as_mut() {
                i.channel = ch;
            }
        }
        self.inp(InCmd::Channel(ch));
    }

    pub fn channel(&self) -> i32 {
        self.inner.ctl.lock().persist.channel
    }

    pub fn set_input_gain_db(&self, db: f32) {
        let g = 10f32.powf(db / 20.0);
        self.inner.ctl.lock().persist.input_gain = g;
        self.inp(InCmd::InputGain(g));
    }

    /// Minimum pluck energy (raise for a microphone in a noisy room).
    pub fn set_onset_floor(&self, f: f32) {
        self.inner.ctl.lock().persist.onset_floor = f;
        self.inp(InCmd::OnsetFloor(f));
    }

    /// Switch tone. Moving a knob within the same kind of tone updates the running chain (tails keep ringing).
    pub fn set_tone(&self, tone: ToneSpec) {
        let (rate, previous) = {
            let mut c = self.inner.ctl.lock();
            let prev = std::mem::replace(&mut c.persist.tone, tone.clone());
            (c.status.input.as_ref().map(|i| i.rate).unwrap_or(c.rate), prev)
        };
        let same_chain = previous.params().is_some() && tone.params().is_some();
        match (same_chain, tone.params()) {
            (true, Some(p)) => self.inp(InCmd::ToneParams(p)),
            _ => self.inp(InCmd::Tone(make_tone(&tone, rate))),
        }
    }

    pub fn tone(&self) -> ToneSpec {
        self.inner.ctl.lock().persist.tone.clone()
    }

    // -- preview clips -------------------------------------------------------------------------------------
    /// Play a clip over everything (replacing any clip playing, which fades out under it). `fade` starts it
    /// with a 250 ms fade-in. Returns the clip's id for `preview_state`.
    pub fn play_preview(&self, clip: Clip, fade: bool, looped: bool) -> u64 {
        let rate = self.rate();
        let clip = if clip.rate == rate || clip.rate == 0 {
            clip
        } else {
            let s = resample(clip.samples, 2, clip.rate, rate).unwrap_or_default();
            Clip { rate, samples: s }
        };
        let mut c = self.inner.ctl.lock();
        c.preview_issued += 1;
        let id = c.preview_issued;
        let sent = match c.out_tx.as_mut() {
            Some(tx) => tx
                .push(OutCmd::Preview {
                    clip: Arc::new(clip),
                    fade,
                    looped,
                    id,
                })
                .is_ok(),
            None => false,
        };
        if !sent {
            self.inner.shared.preview_started.fetch_max(id, Ordering::Relaxed);
        }
        id
    }

    /// Whether the clip `play_preview` returned `id` for hasn't started yet, is playing, or is over (finished,
    /// stopped or replaced).
    pub fn preview_state(&self, id: u64) -> PreviewState {
        let s = &self.inner.shared;
        if s.preview_started.load(Ordering::Acquire) < id {
            PreviewState::Pending
        } else if s.preview_id.load(Ordering::Acquire) == id {
            PreviewState::Playing
        } else {
            PreviewState::Over
        }
    }

    pub fn stop_preview(&self, now: bool) {
        self.out(OutCmd::StopPreview { now });
    }

    pub fn previewing(&self) -> bool {
        self.inner.shared.previewing.load(Ordering::Relaxed)
    }

    /// (seconds into the clip, clip length in seconds).
    pub fn preview_position(&self) -> (f64, f64) {
        let r = self.rate() as f64;
        let s = &self.inner.shared;
        (
            s.preview_pos.load(Ordering::Relaxed) as f64 / r,
            s.preview_len.load(Ordering::Relaxed) as f64 / r,
        )
    }

    // -- guitar capture ------------------------------------------------------------------------------------
    /// Plucks found since the last call.
    pub fn take_onsets(&self) -> Vec<Onset> {
        let mut c = self.inner.ctl.lock();
        let mut out = Vec::new();
        if let Some(rx) = c.onsets_rx.as_mut() {
            while let Ok(o) = rx.pop() {
                out.push(o);
            }
        }
        out
    }

    /// The guitar recording (the last few seconds, addressed by absolute sample index).
    pub fn capture(&self) -> &crate::rt::CaptureRing {
        &self.inner.shared.capture
    }

    pub fn meters(&self) -> Meters {
        let s = &self.inner.shared;
        let mut m = Meters {
            peak: s.in_peak.get(),
            level: s.in_level.get(),
            channel_count: s.in_channels.load(Ordering::Relaxed) as usize,
            output_peak: s.out_peak.get(),
            ..Default::default()
        };
        for (i, c) in s.ch_peaks.iter().enumerate() {
            m.channels[i] = c.get();
        }
        m
    }

    pub fn stats(&self) -> Stats {
        let s = &self.inner.shared;
        Stats {
            dropouts: s.dropouts.load(Ordering::Relaxed),
            monitor_gaps: s.mon_gaps.load(Ordering::Relaxed),
            max_work_ns: s.max_work_ns.load(Ordering::Relaxed),
            in_callbacks: s.in_callbacks.load(Ordering::Relaxed),
            out_callbacks: s.out_callbacks.load(Ordering::Relaxed),
        }
    }

    pub fn reset_stats(&self) {
        let s = &self.inner.shared;
        s.dropouts.store(0, Ordering::Relaxed);
        s.mon_gaps.store(0, Ordering::Relaxed);
        s.max_work_ns.store(0, Ordering::Relaxed);
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.inner.tx.send(Msg::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn resample_song(song: &SongBuffer, rate: u32) -> SongBuffer {
    let f: Vec<f32> = song.samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let r = resample(f, 2, song.rate, rate).unwrap_or_default();
    SongBuffer {
        rate,
        samples: Arc::new(r.iter().map(|&s| crate::decode::f32_to_i16(s)).collect()),
    }
}

// -- the control thread ------------------------------------------------------------------------------------
const CMD_QUEUE: usize = 512;
const MONITOR_QUEUE: usize = 1 << 15;
/// Extra samples held in the guitar cushion between two clocks (measured clean at 128 on the owner's setup).
const CUSHION_MARGIN: usize = 128;
const RETRY: Duration = Duration::from_millis(1500);

struct Running {
    route: Route,
    /// The duplex stream, or the output stream.
    output: Option<Box<dyn Stream>>,
    input: Option<Box<dyn Stream>>,
    duplex: bool,
    trash: Vec<rtrb::Consumer<Trash>>,
    input_failed_at: Option<Instant>,
    output_failed_at: Option<Instant>,
    silent: bool,
}

fn control_loop(inner: Arc<Inner>, rx: Receiver<Msg>, events: Sender<Event>) {
    #[cfg(windows)]
    crate::backend::wasapi::com_init();
    let mut run = Running {
        route: Route::default(),
        output: None,
        input: None,
        duplex: false,
        trash: Vec::new(),
        input_failed_at: None,
        output_failed_at: None,
        silent: true,
    };
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Msg::Open(route)) => {
                close_all(&inner, &mut run);
                run.route = route;
                open_route(&inner, &mut run, &events);
            }
            Ok(Msg::Shutdown) => {
                close_all(&inner, &mut run);
                drain_trash(&mut run);
                return;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(_) => return,
        }
        drain_trash(&mut run);
        supervise(&inner, &mut run, &events);
    }
}

fn drain_trash(run: &mut Running) {
    for t in &mut run.trash {
        while let Ok(item) = t.pop() {
            drop(item);
        }
    }
    run.trash.retain(|t| !t.is_abandoned() || !t.is_empty());
}

fn close_all(inner: &Arc<Inner>, run: &mut Running) {
    // remember where the song was before the output goes away
    run.input = None;
    run.output = None;
    let mut c = inner.ctl.lock();
    c.set_out_tx(&inner.shared, None);
    c.in_tx = None;
    c.onsets_rx = None;
}

fn output_state(inner: &Arc<Inner>, c: &Ctl) -> OutputState {
    let clock = inner.shared.clock.load();
    OutputState {
        song: c.persist.song.clone(),
        stretched: c.persist.stretched.clone(),
        speed: c.persist.speed,
        position: clock.at(now_ns()),
        playing: clock.playing,
        song_gain: c.persist.song_gain,
        guitar_gain: c.persist.guitar_gain,
        monitor: c.persist.monitor,
    }
}

fn input_side(
    inner: &Arc<Inner>,
    persist: &Persist,
    info: &StreamInfo,
    sink: MonitorSink,
    trash: &mut Vec<rtrb::Consumer<Trash>>,
    in_tx: &mut Option<rtrb::Producer<InCmd>>,
    onsets_rx: &mut Option<rtrb::Consumer<Onset>>,
) -> InputSide {
    let (ctx, crx) = rtrb::RingBuffer::new(CMD_QUEUE);
    let (ttx, trx) = rtrb::RingBuffer::new(CMD_QUEUE);
    let (otx, orx) = rtrb::RingBuffer::new(4096);
    trash.push(trx);
    *in_tx = Some(ctx);
    *onsets_rx = Some(orx);
    let mut side = InputSide::new(
        InputConfig {
            rate: info.rate,
            channel: persist.channel,
            monitor: persist.monitor,
            input_gain: persist.input_gain,
            onset_floor: persist.onset_floor,
            max_block: info.max_block.max(info.block).max(4096),
        },
        inner.shared.clone(),
        crx,
        ttx,
        otx,
        sink,
        make_tone(&persist.tone, info.rate),
    );
    side.extra_latency = info.latency;
    side
}

fn output_side(
    inner: &Arc<Inner>,
    state: OutputState,
    info: &StreamInfo,
    monitor: MonitorSource,
    base_channel: usize,
    trash: &mut Vec<rtrb::Consumer<Trash>>,
    out_tx: &mut Option<rtrb::Producer<OutCmd>>,
) -> OutputSide {
    let (ctx, crx) = rtrb::RingBuffer::new(CMD_QUEUE);
    let (ttx, trx) = rtrb::RingBuffer::new(CMD_QUEUE);
    trash.push(trx);
    *out_tx = Some(ctx);
    let mut side = OutputSide::new(
        info.rate,
        state,
        inner.shared.clone(),
        crx,
        ttx,
        monitor,
        info.max_block.max(info.block).max(4096),
    );
    side.base_channel = base_channel;
    side.extra_latency = info.latency;
    side
}

/// Songs and stretched buffers must be at the output rate; resample (or drop the stretch) when it changed.
fn fit_to_rate(c: &mut Ctl, rate: u32) {
    if let Some(s) = &c.persist.song
        && s.rate != rate
    {
        c.persist.song = Some(Arc::new(resample_song(s, rate)));
    }
    if c.persist.stretched.as_ref().is_some_and(|s| s.rate != rate) {
        c.persist.stretched = None;
        c.persist.speed = 1.0;
    }
    c.rate = rate;
}

fn open_route(inner: &Arc<Inner>, run: &mut Running, events: &Sender<Event>) {
    let route = run.route.clone();
    let mut st = Status {
        route: route.clone(),
        ..Default::default()
    };
    run.duplex = false;
    run.input_failed_at = None;
    run.output_failed_at = None;

    // one device both ways (ASIO): one callback, no cushion
    if let (Some(inp), Some(out)) = (&route.input, &route.output)
        && inp.device == *out
        && matches!(out.backend, Backend::Asio | Backend::Null)
    {
        let ireq = InputRequest {
            device: inp.device.clone(),
            buffer: route.buffer,
            exclusive: route.exclusive,
            rate: None,
        };
        let oreq = OutputRequest {
            device: out.clone(),
            buffer: route.buffer,
            exclusive: route.exclusive,
            rate: None,
        };
        let mut guard = inner.ctl.lock();
        let c = &mut *guard;
        let (mut out_tx, mut in_tx, mut onsets_rx) = (None, None, None);
        let mut trash = Vec::new();
        let result = backend::open_duplex(&ireq, &oreq, &mut |iinfo, oinfo| {
            fit_to_rate(c, oinfo.rate);
            let state = output_state(inner, c);
            let input = input_side(
                inner,
                &c.persist,
                iinfo,
                MonitorSink::Duplex,
                &mut trash,
                &mut in_tx,
                &mut onsets_rx,
            );
            let output = output_side(
                inner,
                state,
                oinfo,
                MonitorSource::Duplex,
                route.base_channel,
                &mut trash,
                &mut out_tx,
            );
            Duplex { input, output }
        });
        match result {
            Ok(stream) => {
                st.input = Some(stream.info().clone());
                st.output = Some(stream.info().clone());
                st.duplex = true;
                c.set_out_tx(&inner.shared, out_tx);
                c.in_tx = in_tx;
                c.onsets_rx = onsets_rx;
                c.status = st;
                run.trash.extend(trash);
                run.output = Some(stream);
                run.duplex = true;
                run.silent = false;
                drop(guard);
                let _ = events.send(Event::Changed);
                return;
            }
            Err(e) => {
                st.input_error = Some(format!(
                    "couldn't open {} for input and output: {e}",
                    inp.device.id
                ));
                // fall through: open them separately (or the output alone)
            }
        }
    }

    let prev_err = st.input_error.clone();
    open_output(inner, run, &mut st);
    open_input(inner, run, &mut st);
    if st.input_error.is_none() {
        st.input_error = prev_err.filter(|_| run.input.is_none());
    }
    inner.ctl.lock().status = st;
    let _ = events.send(Event::Changed);
}

fn open_output(inner: &Arc<Inner>, run: &mut Running, st: &mut Status) {
    let route = run.route.clone();
    let mut guard = inner.ctl.lock();
    let c = &mut *guard;
    let mut out_tx = None;
    let mut trash = Vec::new();
    let try_open = |dev: &DeviceId,
                    c: &mut Ctl,
                    out_tx: &mut Option<rtrb::Producer<OutCmd>>,
                    trash: &mut Vec<rtrb::Consumer<Trash>>| {
        let req = OutputRequest {
            device: dev.clone(),
            buffer: route.buffer,
            exclusive: route.exclusive,
            rate: None,
        };
        backend::open_output(&req, &mut |info| {
            fit_to_rate(c, info.rate);
            let state = output_state(inner, c);
            output_side(
                inner,
                state,
                info,
                MonitorSource::None,
                route.base_channel,
                trash,
                out_tx,
            )
        })
    };
    let silent_dev = DeviceId {
        backend: Backend::Null,
        id: "null".into(),
    };
    let mut result = match &route.output {
        Some(dev) => try_open(dev, c, &mut out_tx, &mut trash).map_err(|e| e.to_string()),
        None => Err(String::new()),
    };
    run.silent = false;
    if let Err(e) = &result {
        if route.output.is_some() {
            st.output_error = Some(e.clone());
            run.output_failed_at = Some(Instant::now());
        }
        out_tx = None;
        result = try_open(&silent_dev, c, &mut out_tx, &mut trash).map_err(|e| e.to_string());
        run.silent = true;
    }
    if let Ok(stream) = result {
        st.output = Some(stream.info().clone());
        st.silent_output = run.silent;
        c.set_out_tx(&inner.shared, out_tx);
        run.trash.extend(trash);
        run.output = Some(stream);
    }
}

fn open_input(inner: &Arc<Inner>, run: &mut Running, st: &mut Status) {
    let route = run.route.clone();
    let Some(inp) = &route.input else { return };
    let mut guard = inner.ctl.lock();
    let c = &mut *guard;
    let out_rate = c.rate;
    let out_block = st.output.as_ref().map(|o| o.block).unwrap_or(256);
    let (mon_tx, mon_rx) = rtrb::RingBuffer::<f32>::new(MONITOR_QUEUE);
    let (mut in_tx, mut onsets_rx) = (None, None);
    let mut trash = Vec::new();
    let mut in_block = 0usize;
    let req = InputRequest {
        device: inp.device.clone(),
        buffer: route.buffer,
        exclusive: route.exclusive,
        rate: Some(out_rate),
    };
    let mut mon_tx = Some(mon_tx);
    let result = backend::open_input(&req, &mut |info| {
        in_block = info.block;
        let sink = MonitorSink::Queue {
            tx: mon_tx.take().expect("one input side"),
            resampler: LinearResampler::new(info.rate, out_rate),
        };
        input_side(
            inner,
            &c.persist,
            info,
            sink,
            &mut trash,
            &mut in_tx,
            &mut onsets_rx,
        )
    });
    match result {
        Ok(stream) => {
            // hand the cushion to the output side, sized from the block the input really delivers
            let in_block = if stream.info().block > 0 {
                stream.info().block
            } else {
                in_block
            };
            let in_block_out =
                (in_block as f64 * out_rate as f64 / stream.info().rate as f64).ceil() as usize;
            let target = in_block_out + out_block + CUSHION_MARGIN;
            let cushion = Cushion::new(mon_rx, target, out_block.max(4096));
            if let Some(tx) = c.out_tx.as_mut() {
                let _ = tx.push(OutCmd::Cushion(Some(Box::new(cushion))));
            }
            st.input = Some(stream.info().clone());
            st.input_error = None;
            c.in_tx = in_tx;
            c.onsets_rx = onsets_rx;
            run.trash.extend(trash);
            run.input = Some(stream);
        }
        Err(e) => {
            st.input_error = Some(e.to_string());
            run.input_failed_at = Some(Instant::now());
        }
    }
}

/// Watch for failed streams and bring them back.
fn supervise(inner: &Arc<Inner>, run: &mut Running, events: &Sender<Event>) {
    // a stream that reported failure
    if let Some(s) = &run.output
        && let Some(msg) = s.failure()
    {
        let name = s.info().device_name.clone();
        run.output = None;
        if run.duplex {
            run.input = None;
        }
        let _ = events.send(Event::OutputLost(format!("{name}: {msg}")));
        if run.duplex {
            let _ = events.send(Event::InputLost(format!("{name}: {msg}")));
        }
        let mut st = inner.ctl.lock().status.clone();
        st.output_error = Some(msg);
        st.input = if run.duplex { None } else { st.input };
        open_output_fallback(inner, run, &mut st);
        inner.ctl.lock().status = st;
        run.output_failed_at = Some(Instant::now());
        if run.duplex {
            run.input_failed_at = Some(Instant::now());
        }
        let _ = events.send(Event::Changed);
    }
    if let Some(s) = &run.input
        && let Some(msg) = s.failure()
    {
        let name = s.info().device_name.clone();
        run.input = None;
        {
            let mut c = inner.ctl.lock();
            c.in_tx = None;
            if let Some(tx) = c.out_tx.as_mut() {
                let _ = tx.push(OutCmd::Cushion(None));
            }
            c.status.input = None;
            c.status.input_error = Some(msg.clone());
        }
        run.input_failed_at = Some(Instant::now());
        let _ = events.send(Event::InputLost(format!("{name}: {msg}")));
        let _ = events.send(Event::Changed);
    }
    // retry what's missing
    let now = Instant::now();
    let due = |t: Option<Instant>| t.map(|t| now.duration_since(t) >= RETRY).unwrap_or(false);
    if run.silent && run.route.output.is_some() && due(run.output_failed_at) {
        let mut st = inner.ctl.lock().status.clone();
        let had_input = run.input.is_some();
        if run.duplex || matches!(run.route.output.as_ref().map(|d| d.backend), Some(Backend::Asio)) {
            // a duplex route reopens as a whole
            close_all(inner, run);
            open_route(inner, run, events);
            if !run.silent {
                let _ = events.send(Event::OutputBack(
                    st.output.map(|o| o.device_name).unwrap_or_default(),
                ));
            }
            return;
        }
        run.output = None;
        st.output_error = None;
        open_output(inner, run, &mut st);
        if !run.silent {
            // the new output side needs the guitar cushion again
            run.input = None;
            if had_input {
                open_input(inner, run, &mut st);
            }
            let _ = events.send(Event::OutputBack(
                st.output.clone().map(|o| o.device_name).unwrap_or_default(),
            ));
        } else {
            run.output_failed_at = Some(now);
        }
        inner.ctl.lock().status = st;
        let _ = events.send(Event::Changed);
    }
    if run.input.is_none() && run.route.input.is_some() && !run.duplex && due(run.input_failed_at) {
        let mut st = inner.ctl.lock().status.clone();
        open_input(inner, run, &mut st);
        if run.input.is_some() {
            let _ = events.send(Event::InputBack(
                st.input.clone().map(|i| i.device_name).unwrap_or_default(),
            ));
        } else {
            run.input_failed_at = Some(now);
        }
        inner.ctl.lock().status = st;
        let _ = events.send(Event::Changed);
    }
}

fn open_output_fallback(inner: &Arc<Inner>, run: &mut Running, st: &mut Status) {
    let real = run.route.output.take();
    open_output(inner, run, st);
    run.route.output = real;
    run.silent = true;
    st.silent_output = true;
}
