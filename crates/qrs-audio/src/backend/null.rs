//! A timer instead of a sound card. Ids: "null" (silence in, discarded out), "null:sine" (input plays a plucked
//! note every second, for tests and the CLI's self-test), "null:stand-in" (screenshot mode's guitar, below). Also
//! the engine's silent fallback output, so the song clock keeps running when no output device is available.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use super::{
    Backend, BufferSize, DeviceId, DeviceInfo, Failure, Hardware, InputRequest, MakeDuplex, MakeInput,
    MakeOutput, OpenError, OutputRequest, Stream, StreamInfo,
};
use crate::clock::now_ns;

pub fn enumerate() -> Vec<DeviceInfo> {
    let mut ids = vec!["null", "null:sine"];
    if STAND_IN_LISTED.load(Ordering::Acquire) {
        ids.push(STAND_IN);
    }
    ids.iter()
        .map(|id| DeviceInfo {
            id: DeviceId {
                backend: Backend::Null,
                id: id.to_string(),
            },
            name: match *id {
                "null" => "No sound (silent)".into(),
                STAND_IN => stand_in_name(),
                _ => "Test signal".into(),
            },
            input_channels: 2,
            output_channels: 2,
            input_channel_names: vec![],
            default_rate: 48000,
            is_default_input: false,
            is_default_output: false,
            hardware: Hardware::default(),
        })
        .collect()
}

fn block_of(b: BufferSize) -> usize {
    match b {
        BufferSize::Frames(n) => (n as usize).clamp(16, 8192),
        BufferSize::Default => 256,
    }
}

struct NullStream {
    info: StreamInfo,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    failure: Failure,
}

impl Stream for NullStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }
    fn failure(&self) -> Option<String> {
        self.failure.get()
    }
}

impl Drop for NullStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Screenshot mode's stand-in for the guitar interface, as the Python app's screenshot tool fakes it
/// (`tools/screenshots.py` FakeEngine and FakeRing): a steady Eb3 three cents sharp on the first channel, under the
/// name "NUX Audio (ASIO)" with its latencies. Listed only after `enable_stand_in` (quickrs --screenshots); its
/// output side, like "null", is heard by no one.
pub const STAND_IN: &str = "null:stand-in";
pub const STAND_IN_NAME: &str = "NUX Audio (ASIO)";
static STAND_IN_LISTED: AtomicBool = AtomicBool::new(false);

/// The stand-in's name: STAND_IN_NAME, or QUICKRS_SHOT_DEVICE (the shared screenshots show a Real Tone Cable).
pub fn stand_in_name() -> String {
    std::env::var("QUICKRS_SHOT_DEVICE").unwrap_or_else(|_| STAND_IN_NAME.into())
}

pub fn enable_stand_in() {
    STAND_IN_LISTED.store(true, Ordering::Release);
}

/// The stand-in guitar: 0.2 x (0.6 sin(f0) + sin(2 f0)) at Eb3 + 3 cents, like FakeRing.
struct StandIn {
    rate: f64,
    n: u64,
}

impl StandIn {
    fn next(&mut self) -> f32 {
        let f0 = 440.0 * 2f64.powf((51.0 - 69.0) / 12.0) * 2f64.powf(3.0 / 1200.0);
        let w = std::f64::consts::TAU * f0 * self.n as f64 / self.rate;
        self.n += 1;
        (0.2 * (0.6 * w.sin() + (2.0 * w).sin())) as f32
    }
}

/// The synthetic guitar: a decaying plucked tone (110 Hz with harmonics) every second.
struct TestSignal {
    rate: f64,
    n: u64,
}

impl TestSignal {
    fn next(&mut self) -> f32 {
        let t = (self.n % self.rate as u64) as f64 / self.rate;
        self.n += 1;
        let env = (-t * 6.0).exp();
        let w = std::f64::consts::TAU * 110.0 * t;
        (0.35 * env * (w.sin() + 0.5 * (2.0 * w).sin() + 0.25 * (3.0 * w).sin())) as f32
    }
}

fn info(id: &DeviceId, rate: u32, block: usize, mode: &str) -> StreamInfo {
    StreamInfo {
        device: id.clone(),
        device_name: match id.id.as_str() {
            "null" => "No sound".into(),
            STAND_IN => stand_in_name(),
            _ => "Test signal".into(),
        },
        rate,
        channels: 2,
        block,
        max_block: block,
        latency: 0.0,
        mode: mode.into(),
    }
}

/// The stand-in's stream info: the Python screenshot tool's latencies (2.7 ms in, 5.3 ms out) and ASIO.
fn stand_in_info(mut i: StreamInfo, input: bool) -> StreamInfo {
    if i.device.id == STAND_IN {
        i.latency = if input { 0.0027 } else { 0.0053 };
        i.mode = "ASIO".into();
    }
    i
}

/// Run `tick(first_frame_ns)` every block period on a thread, on a real-time schedule (catching up when late).
fn spawn(
    rate: u32,
    block: usize,
    stop: Arc<AtomicBool>,
    mut tick: impl FnMut(u64) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("qrs-null-audio".into())
        .spawn(move || {
            let period = block as f64 / rate as f64 * 1e9;
            let start = now_ns() as f64;
            let mut k: u64 = 0;
            while !stop.load(Ordering::Acquire) {
                let due = start + k as f64 * period;
                let now = now_ns() as f64;
                if now < due {
                    std::thread::sleep(Duration::from_nanos((due - now) as u64));
                    continue;
                }
                tick(due as u64);
                k += 1;
            }
        })
        .expect("spawn null audio thread")
}

pub fn open_input(req: &InputRequest, make: MakeInput) -> Result<Box<dyn Stream>, OpenError> {
    let rate = req.rate.unwrap_or(48000);
    let block = block_of(req.buffer);
    let info = stand_in_info(info(&req.device, rate, block, "None"), true);
    let mut side = make(&info);
    let stop = Arc::new(AtomicBool::new(false));
    let sine = req.device.id == "null:sine";
    let stand_in = req.device.id == STAND_IN;
    let mut sig = TestSignal {
        rate: rate as f64,
        n: 0,
    };
    let mut guitar = StandIn {
        rate: rate as f64,
        n: 0,
    };
    let mut buf = vec![0.0f32; block * 2];
    let period_ns = (block as f64 / rate as f64 * 1e9) as u64;
    let thread = spawn(rate, block, stop.clone(), move |due| {
        if sine {
            for f in buf.chunks_exact_mut(2) {
                let s = sig.next();
                f[0] = s;
                f[1] = s * 0.5;
            }
        } else if stand_in {
            for f in buf.chunks_exact_mut(2) {
                f[0] = guitar.next();
            }
        }
        // the block was captured over the period that just ended
        side.process(&buf, 2, due.saturating_sub(period_ns));
    });
    Ok(Box::new(NullStream {
        info,
        stop,
        thread: Some(thread),
        failure: Failure::default(),
    }))
}

pub fn open_output(req: &OutputRequest, make: MakeOutput) -> Result<Box<dyn Stream>, OpenError> {
    let rate = req.rate.unwrap_or(48000);
    let block = block_of(req.buffer);
    let info = stand_in_info(info(&req.device, rate, block, "None"), false);
    let mut side = make(&info);
    let stop = Arc::new(AtomicBool::new(false));
    let mut buf = vec![0.0f32; block * 2];
    let thread = spawn(rate, block, stop.clone(), move |due| {
        side.process(&mut buf, 2, due)
    });
    Ok(Box::new(NullStream {
        info,
        stop,
        thread: Some(thread),
        failure: Failure::default(),
    }))
}

pub fn open_duplex(
    inp: &InputRequest,
    out: &OutputRequest,
    make: MakeDuplex,
) -> Result<Box<dyn Stream>, OpenError> {
    let rate = out.rate.or(inp.rate).unwrap_or(48000);
    let block = block_of(out.buffer);
    let in_info = stand_in_info(info(&inp.device, rate, block, "None"), true);
    let out_info = stand_in_info(info(&out.device, rate, block, "None"), false);
    let mut d = make(&in_info, &out_info);
    let stop = Arc::new(AtomicBool::new(false));
    let sine = inp.device.id == "null:sine";
    let stand_in = inp.device.id == STAND_IN;
    let mut sig = TestSignal {
        rate: rate as f64,
        n: 0,
    };
    let mut guitar = StandIn {
        rate: rate as f64,
        n: 0,
    };
    let mut ibuf = vec![0.0f32; block * 2];
    let mut obuf = vec![0.0f32; block * 2];
    let period_ns = (block as f64 / rate as f64 * 1e9) as u64;
    let thread = spawn(rate, block, stop.clone(), move |due| {
        if sine {
            for f in ibuf.chunks_exact_mut(2) {
                let s = sig.next();
                f[0] = s;
                f[1] = s * 0.5;
            }
        } else if stand_in {
            for f in ibuf.chunks_exact_mut(2) {
                f[0] = guitar.next();
            }
        }
        d.process(&ibuf, 2, &mut obuf, 2, due.saturating_sub(period_ns), due);
    });
    Ok(Box::new(NullStream {
        info: out_info,
        stop,
        thread: Some(thread),
        failure: Failure::default(),
    }))
}
