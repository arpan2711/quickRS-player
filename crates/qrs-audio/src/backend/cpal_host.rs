//! Streams through cpal, shared by the ASIO backend and the System (default host) backend.
//!
//! cpal streams can't move between threads, so each one lives on its own thread: the thread finds the device and
//! picks the format, reports it, receives the real-time side built for it, builds and starts the stream, and
//! waits until it's told to stop. A duplex stream is an input and an output stream on the same ASIO driver: the
//! driver calls both from its buffer switch, input first, so the guitar block goes straight to the output.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample as _, SampleFormat};
use crossbeam_channel::bounded;
use parking_lot::Mutex;

use super::{
    Backend, BufferSize, DeviceId, DeviceInfo, Failure, Hardware, OpenError, Stream, StreamInfo, oerr,
};
use crate::clock::now_ns;
use crate::rt::Duplex;
use crate::rt::input::InputSide;
use crate::rt::output::OutputSide;

/// The device id meaning "whatever the system default is right now".
pub const DEFAULT: &str = "default";

pub fn host(id: Option<cpal::HostId>) -> Result<cpal::Host, String> {
    match id {
        Some(h) => cpal::host_from_id(h).map_err(|e| e.to_string()),
        None => Ok(cpal::default_host()),
    }
}

fn device_name(d: &cpal::Device) -> String {
    d.description().map(|x| x.name().to_string()).unwrap_or_default()
}

/// Devices of a host. With `defaults_only`, just the two "System default" entries.
pub fn enumerate(host_id: Option<cpal::HostId>, backend: Backend, defaults_only: bool) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    let Ok(h) = host(host_id) else { return out };
    if backend == Backend::System {
        for (input, d) in [
            (true, h.default_input_device()),
            (false, h.default_output_device()),
        ] {
            let Some(d) = d else { continue };
            let cfg = if input {
                d.default_input_config()
            } else {
                d.default_output_config()
            };
            let (ch, rate) = cfg.map(|c| (c.channels(), c.sample_rate())).unwrap_or((2, 48000));
            out.push(DeviceInfo {
                id: DeviceId {
                    backend,
                    id: if input { "default-in" } else { "default-out" }.into(),
                },
                name: format!(
                    "System default {} ({})",
                    if input { "input" } else { "output" },
                    device_name(&d)
                ),
                input_channels: if input { ch } else { 0 },
                output_channels: if input { 0 } else { ch },
                input_channel_names: vec![],
                default_rate: rate,
                is_default_input: input,
                is_default_output: !input,
                hardware: Hardware::default(),
            });
        }
        if defaults_only {
            return out;
        }
    }
    let Ok(devices) = h.devices() else { return out };
    for d in devices {
        let name = device_name(&d);
        if name.is_empty() {
            continue;
        }
        let inp = d.default_input_config().ok();
        let outp = d.default_output_config().ok();
        if inp.is_none() && outp.is_none() {
            continue;
        }
        out.push(DeviceInfo {
            id: DeviceId {
                backend,
                id: name.clone(),
            },
            name,
            input_channels: inp.as_ref().map(|c| c.channels()).unwrap_or(0),
            output_channels: outp.as_ref().map(|c| c.channels()).unwrap_or(0),
            input_channel_names: vec![],
            default_rate: inp
                .as_ref()
                .or(outp.as_ref())
                .map(|c| c.sample_rate())
                .unwrap_or(48000),
            is_default_input: false,
            is_default_output: false,
            hardware: Hardware::default(),
        });
    }
    out
}

fn find_device(h: &cpal::Host, id: &str, input: bool) -> Option<cpal::Device> {
    match id {
        "default-in" => h.default_input_device(),
        "default-out" => h.default_output_device(),
        DEFAULT => {
            if input {
                h.default_input_device()
            } else {
                h.default_output_device()
            }
        }
        name => h.devices().ok()?.find(|d| device_name(d) == name),
    }
}

struct CpalStream {
    info: StreamInfo,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    failure: Failure,
}

impl Stream for CpalStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }
    fn failure(&self) -> Option<String> {
        self.failure.get()
    }
}

impl Drop for CpalStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

/// Pick a stream config: the device's default, at the wanted rate if the device offers it, with all of the
/// device's channels (inputs) or enough for the mix (outputs), at the requested buffer size when supported.
fn pick_config(
    d: &cpal::Device,
    input: bool,
    buffer: BufferSize,
    rate: Option<u32>,
    min_channels: u16,
) -> Result<(cpal::StreamConfig, SampleFormat), String> {
    let def = if input {
        d.default_input_config()
    } else {
        d.default_output_config()
    }
    .map_err(|e| e.to_string())?;
    let mut fmt = def.sample_format();
    let mut cfg = def.config();
    let ranges: Vec<cpal::SupportedStreamConfigRange> = if input {
        d.supported_input_configs()
            .map(|c| c.collect())
            .unwrap_or_default()
    } else {
        d.supported_output_configs()
            .map(|c| c.collect())
            .unwrap_or_default()
    };
    if let Some(r) = rate
        && r != cfg.sample_rate
        && let Some(range) = ranges
            .iter()
            .find(|c| c.channels() == cfg.channels && c.min_sample_rate() <= r && r <= c.max_sample_rate())
    {
        cfg.sample_rate = r;
        fmt = range.sample_format();
    }
    if cfg.channels < min_channels
        && let Some(range) = ranges
            .iter()
            .filter(|c| c.channels() >= min_channels)
            .min_by_key(|c| c.channels())
    {
        cfg.channels = range.channels();
        fmt = range.sample_format();
    }
    cfg.buffer_size = match buffer {
        BufferSize::Frames(n) => {
            let ok = ranges.iter().any(|c| match c.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } => *min <= n && n <= *max,
                cpal::SupportedBufferSize::Unknown => true,
            });
            if ok {
                cpal::BufferSize::Fixed(n)
            } else {
                cpal::BufferSize::Default
            }
        }
        BufferSize::Default => cpal::BufferSize::Default,
    };
    Ok((cfg, fmt))
}

fn block_guess(cfg: &cpal::StreamConfig) -> usize {
    match cfg.buffer_size {
        cpal::BufferSize::Fixed(n) => n as usize,
        cpal::BufferSize::Default => 512,
    }
}

fn info_of(
    backend: Backend,
    id: &str,
    name: &str,
    cfg: &cpal::StreamConfig,
    fmt: SampleFormat,
    mode: &str,
) -> StreamInfo {
    let block = block_guess(cfg);
    StreamInfo {
        device: DeviceId {
            backend,
            id: id.to_string(),
        },
        device_name: name.to_string(),
        rate: cfg.sample_rate,
        channels: cfg.channels,
        block,
        max_block: block.max(8192),
        latency: 0.0,
        mode: format!("{mode} ({} Hz, {} ch, {fmt:?})", cfg.sample_rate, cfg.channels),
    }
}

/// Convert a cpal buffer of any sample type to f32.
fn to_f32(data: &cpal::Data, out: &mut Vec<f32>) {
    fn conv<T: cpal::SizedSample>(src: Option<&[T]>, out: &mut Vec<f32>)
    where
        f32: FromSample<T>,
    {
        if let Some(src) = src {
            out.clear();
            out.extend(src.iter().map(|&s| f32::from_sample(s)));
        }
    }
    match data.sample_format() {
        SampleFormat::F32 => conv::<f32>(data.as_slice(), out),
        SampleFormat::I16 => conv::<i16>(data.as_slice(), out),
        SampleFormat::I32 => conv::<i32>(data.as_slice(), out),
        SampleFormat::I24 => conv::<cpal::I24>(data.as_slice(), out),
        SampleFormat::U16 => conv::<u16>(data.as_slice(), out),
        SampleFormat::I8 => conv::<i8>(data.as_slice(), out),
        SampleFormat::U8 => conv::<u8>(data.as_slice(), out),
        SampleFormat::F64 => conv::<f64>(data.as_slice(), out),
        _ => out.clear(),
    }
}

/// Write f32 into a cpal buffer of any sample type.
fn from_f32(src: &[f32], data: &mut cpal::Data) {
    fn conv<T: cpal::SizedSample + FromSample<f32>>(src: &[f32], dst: Option<&mut [T]>) {
        if let Some(dst) = dst {
            for (d, s) in dst.iter_mut().zip(src) {
                *d = T::from_sample(*s);
            }
        }
    }
    match data.sample_format() {
        SampleFormat::F32 => conv::<f32>(src, data.as_slice_mut()),
        SampleFormat::I16 => conv::<i16>(src, data.as_slice_mut()),
        SampleFormat::I32 => conv::<i32>(src, data.as_slice_mut()),
        SampleFormat::I24 => conv::<cpal::I24>(src, data.as_slice_mut()),
        SampleFormat::U16 => conv::<u16>(src, data.as_slice_mut()),
        SampleFormat::I8 => conv::<i8>(src, data.as_slice_mut()),
        SampleFormat::U8 => conv::<u8>(src, data.as_slice_mut()),
        SampleFormat::F64 => conv::<f64>(src, data.as_slice_mut()),
        _ => {}
    }
}

fn capture_ns(info: &cpal::InputCallbackInfo) -> u64 {
    let t = info.timestamp();
    now_ns().saturating_sub(t.callback.duration_since(t.capture).as_nanos() as u64)
}

fn playback_ns(info: &cpal::OutputCallbackInfo) -> u64 {
    let t = info.timestamp();
    now_ns() + t.playback.duration_since(t.callback).as_nanos() as u64
}

/// What a stream thread builds: an input, an output, or both (duplex). (Sent once per stream: size is fine.)
#[allow(clippy::large_enum_variant)]
pub enum Side {
    Input(InputSide),
    Output(OutputSide),
    Duplex(Duplex),
}

pub struct Open {
    pub host: Option<cpal::HostId>,
    pub backend: Backend,
    pub mode: &'static str,
    pub id: String,
    pub input: bool,
    pub output: bool,
    pub buffer: BufferSize,
    pub rate: Option<u32>,
    pub out_channels: u16,
}

/// Open a stream on its own thread. `make` gets the input and/or output format and returns the side to run.
pub fn open(
    o: Open,
    make: &mut dyn FnMut(Option<&StreamInfo>, Option<&StreamInfo>) -> Side,
) -> Result<Box<dyn Stream>, OpenError> {
    let (info_tx, info_rx) = bounded::<Result<(Option<StreamInfo>, Option<StreamInfo>), String>>(1);
    let (side_tx, side_rx) = bounded::<Side>(1);
    let (built_tx, built_rx) = bounded::<Result<Option<u32>, String>>(1);
    let stop = Arc::new(AtomicBool::new(false));
    let failure = Failure::default();
    let (stop2, fail2) = (stop.clone(), failure.clone());
    let thread = std::thread::Builder::new()
        .name(format!("qrs-{}", o.mode.to_lowercase()))
        .spawn(move || {
            let setup = (|| -> Result<_, String> {
                let h = host(o.host)?;
                let dev =
                    find_device(&h, &o.id, o.input).ok_or_else(|| format!("{} isn't connected", o.id))?;
                let name = device_name(&dev);
                let inc = if o.input {
                    Some(pick_config(&dev, true, o.buffer, o.rate, 1)?)
                } else {
                    None
                };
                let outc = if o.output {
                    // a duplex output runs at the input's rate (one driver, one clock)
                    let rate = inc.as_ref().map(|c| c.0.sample_rate).or(o.rate);
                    Some(pick_config(&dev, false, o.buffer, rate, o.out_channels)?)
                } else {
                    None
                };
                Ok((dev, name, inc, outc))
            })();
            let (dev, name, inc, outc) = match setup {
                Ok(s) => s,
                Err(e) => {
                    let _ = info_tx.send(Err(e));
                    return;
                }
            };
            let ii = inc
                .as_ref()
                .map(|(c, f)| info_of(o.backend, &o.id, &name, c, *f, o.mode));
            let oi = outc
                .as_ref()
                .map(|(c, f)| info_of(o.backend, &o.id, &name, c, *f, o.mode));
            let _ = info_tx.send(Ok((ii, oi)));
            let Ok(side) = side_rx.recv() else { return };
            let f_in = fail2.clone();
            let f_out = fail2.clone();
            let on_err = move |f: Failure| move |e: cpal::Error| f.fail(e.to_string());
            let timeout = Some(Duration::from_secs(5));
            let mut streams = Vec::new();
            let result = (|| -> Result<Option<u32>, String> {
                match side {
                    Side::Input(mut s) => {
                        let (cfg, fmt) = inc.ok_or("no input config")?;
                        let ch = cfg.channels as usize;
                        let mut buf = Vec::with_capacity(16384);
                        streams.push(
                            dev.build_input_stream_raw(
                                cfg,
                                fmt,
                                move |data: &cpal::Data, info| {
                                    to_f32(data, &mut buf);
                                    s.process(&buf, ch, capture_ns(info));
                                },
                                on_err(f_in),
                                timeout,
                            )
                            .map_err(|e| e.to_string())?,
                        );
                    }
                    Side::Output(mut s) => {
                        let (cfg, fmt) = outc.ok_or("no output config")?;
                        let ch = cfg.channels as usize;
                        let mut buf = vec![0.0f32; 16384];
                        streams.push(
                            dev.build_output_stream_raw(
                                cfg,
                                fmt,
                                move |data: &mut cpal::Data, info| {
                                    let n = data.len().min(buf.len());
                                    s.process(&mut buf[..n], ch, playback_ns(info));
                                    from_f32(&buf[..n], data);
                                },
                                on_err(f_out),
                                timeout,
                            )
                            .map_err(|e| e.to_string())?,
                        );
                    }
                    Side::Duplex(d) => {
                        let (icfg, ifmt) = inc.ok_or("no input config")?;
                        let (ocfg, ofmt) = outc.ok_or("no output config")?;
                        let (ich, och) = (icfg.channels as usize, ocfg.channels as usize);
                        // both callbacks run on the driver's thread one after the other: the lock is never contended
                        let shared = Arc::new(Mutex::new((d, Vec::<f32>::with_capacity(16384), 0u64)));
                        let s_in = shared.clone();
                        let mut conv = Vec::with_capacity(16384);
                        streams.push(
                            dev.build_input_stream_raw(
                                icfg,
                                ifmt,
                                move |data: &cpal::Data, info| {
                                    to_f32(data, &mut conv);
                                    if let Some(mut g) = s_in.try_lock() {
                                        let (_, pending, t) = &mut *g;
                                        pending.clear();
                                        pending.extend_from_slice(&conv);
                                        *t = capture_ns(info);
                                    }
                                },
                                on_err(f_in),
                                timeout,
                            )
                            .map_err(|e| e.to_string())?,
                        );
                        let mut obuf = vec![0.0f32; 16384];
                        streams.push(
                            dev.build_output_stream_raw(
                                ocfg,
                                ofmt,
                                move |data: &mut cpal::Data, info| {
                                    let n = data.len().min(obuf.len());
                                    let play = playback_ns(info);
                                    match shared.try_lock() {
                                        Some(mut g) => {
                                            let (d, pending, t) = &mut *g;
                                            let frames = n / och;
                                            let need = frames * ich;
                                            if pending.len() < need {
                                                pending.resize(need, 0.0);
                                            }
                                            d.process(&pending[..need], ich, &mut obuf[..n], och, *t, play);
                                            pending.clear();
                                        }
                                        None => obuf[..n].fill(0.0),
                                    }
                                    from_f32(&obuf[..n], data);
                                },
                                on_err(f_out),
                                timeout,
                            )
                            .map_err(|e| e.to_string())?,
                        );
                    }
                }
                for s in &streams {
                    s.play().map_err(|e| e.to_string())?;
                }
                // the buffer the driver actually uses (the default is the driver's own setting)
                Ok(streams.first().and_then(|s| s.buffer_size().ok()))
            })();
            let ok = result.is_ok();
            let _ = built_tx.send(result);
            if ok {
                while !stop2.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_millis(200));
                }
            }
            drop(streams);
        })
        .map_err(oerr)?;
    let (ii, oi) = match info_rx.recv() {
        Ok(Ok(x)) => x,
        Ok(Err(e)) => {
            let _ = thread.join();
            return Err(oerr(e));
        }
        Err(_) => {
            let _ = thread.join();
            return Err(oerr("the audio thread ended during setup"));
        }
    };
    let side = make(ii.as_ref(), oi.as_ref());
    if side_tx.send(side).is_err() {
        return Err(oerr("the audio thread ended during setup"));
    }
    let actual = match built_rx.recv() {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            let _ = thread.join();
            return Err(oerr(e));
        }
        Err(_) => return Err(oerr("the audio thread ended during setup")),
    };
    let mut info = oi.or(ii).expect("an input or an output");
    if let Some(n) = actual.filter(|n| *n > 0) {
        info.block = n as usize;
    }
    Ok(Box::new(CpalStream {
        info,
        stop,
        thread: Some(thread),
        failure,
    }))
}
