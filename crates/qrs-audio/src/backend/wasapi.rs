//! WASAPI (Windows): endpoint enumeration with hardware ids, shared and exclusive streams, hot-plug.
//! Our own implementation on the `windows` crate (cpal's WASAPI has no exclusive mode and no device ids).
//!
//! Hardware ids: every endpoint's property store holds its PnP instance path as "{1}.<path>" under
//! {b3f8fa53-0004-438e-9003-51a46e139bfc},2 (e.g. "{1}.USB\VID_291A&PID_3369&MI_02\..."). That is the field
//! Rocksmith checks for the Real Tone Cable (VID_12BA&PID_00FF), and the one RS_ASIO fakes for ASIO inputs.
//!
//! Streams: each runs on its own thread with MMCSS "Pro Audio" priority, event-driven. The thread sets the
//! client up, reports the format back, receives the real-time side built for that format, then runs. Shared
//! output keeps only one engine period plus a small margin queued (cpal keeps the whole buffer full, ~30 ms);
//! shared streams use the device's smallest period (IAudioClient3) when it has one. Capture blocks are stamped
//! with WASAPI's own QPC capture time.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded};
use windows::Win32::Foundation::{CloseHandle, HANDLE, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::{GUID, HSTRING, Interface, PCWSTR, PWSTR, implement};

use super::{
    Backend, BufferSize, Bus, DeviceEvent, DeviceId, DeviceInfo, Failure, Hardware, InputRequest, MakeInput,
    MakeOutput, OpenError, OutputRequest, Stream, StreamInfo, oerr,
};
use crate::clock::now_ns;
use crate::rt::input::InputSide;
use crate::rt::output::OutputSide;

/// Initialise COM on the calling thread (multithreaded apartment). Harmless if already initialised.
pub fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { p.to_string().unwrap_or_default() };
    unsafe { CoTaskMemFree(Some(p.0 as _)) };
    s
}

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    com_init();
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

const fn key(fmtid: u128, pid: u32) -> PROPERTYKEY {
    PROPERTYKEY {
        fmtid: GUID::from_u128(fmtid),
        pid,
    }
}
/// "Line (NUX Audio)"
const PKEY_FRIENDLY_NAME: PROPERTYKEY = key(0xa45c254e_df1c_4efd_8020_67d146a850e0, 14);
/// "Line", "Microphone", "Speakers"
const PKEY_DEVICE_DESC: PROPERTYKEY = key(0xa45c254e_df1c_4efd_8020_67d146a850e0, 2);
/// "USB", "HDAUDIO", "ROOT", "TUSBAUDIO_ENUM", "BTHENUM"
const PKEY_ENUMERATOR: PROPERTYKEY = key(0xa45c254e_df1c_4efd_8020_67d146a850e0, 24);
/// The adapter's name: "NUX Audio", "Rocksmith Guitar Adapter"
const PKEY_ADAPTER_NAME: PROPERTYKEY = key(0xb3f8fa53_0004_438e_9003_51a46e139bfc, 6);
/// "{1}.<PnP instance path>"
const PKEY_INSTANCE_PATH: PROPERTYKEY = key(0xb3f8fa53_0004_438e_9003_51a46e139bfc, 2);

fn prop_string(store: &IPropertyStore, k: &PROPERTYKEY) -> Option<String> {
    unsafe {
        let mut v = store.GetValue(k).ok()?;
        let s = PropVariantToStringAlloc(&v).ok().map(pwstr);
        let _ = PropVariantClear(&mut v);
        s.filter(|s| !s.is_empty())
    }
}

/// Every string property of every active endpoint (diagnostics: `quickrs-cli devices --raw`).
pub fn dump_properties() -> Vec<(String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let Ok(en) = enumerator() else { return out };
    unsafe {
        let Ok(col) = en.EnumAudioEndpoints(eAll, DEVICE_STATE_ACTIVE) else {
            return out;
        };
        for i in 0..col.GetCount().unwrap_or(0) {
            let Ok(dev) = col.Item(i) else { continue };
            let id = dev.GetId().map(pwstr).unwrap_or_default();
            let mut props = Vec::new();
            if let Ok(store) = dev.OpenPropertyStore(STGM_READ) {
                for k in 0..store.GetCount().unwrap_or(0) {
                    let mut pk = PROPERTYKEY::default();
                    if store.GetAt(k, &mut pk).is_err() {
                        continue;
                    }
                    if let Some(v) = prop_string(&store, &pk) {
                        props.push((format!("{:?},{}", pk.fmtid, pk.pid), v));
                    }
                }
            }
            out.push((id, props));
        }
    }
    out
}

// --- formats ------------------------------------------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sample {
    F32,
    I16,
    I24,
    I32,
}

impl Sample {
    fn bytes(self) -> usize {
        match self {
            Sample::I16 => 2,
            Sample::I24 => 3,
            Sample::F32 | Sample::I32 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Fmt {
    rate: u32,
    channels: u16,
    sample: Sample,
}

impl Fmt {
    fn frame_bytes(&self) -> usize {
        self.sample.bytes() * self.channels as usize
    }
    fn describe(&self) -> String {
        let s = match self.sample {
            Sample::F32 => "float",
            Sample::I16 => "16-bit",
            Sample::I24 => "24-bit",
            Sample::I32 => "32-bit",
        };
        format!("{} Hz, {} ch, {s}", self.rate, self.channels)
    }
}

/// # Safety
/// `p` points at a valid WAVEFORMATEX (or WAVEFORMATEXTENSIBLE when tagged so).
unsafe fn parse_format(p: *const WAVEFORMATEX) -> Option<Fmt> {
    let wf: WAVEFORMATEX = unsafe { std::ptr::read_unaligned(p) };
    let tag = wf.wFormatTag as u32;
    let bits = wf.wBitsPerSample;
    let (float, pcm) = if tag == WAVE_FORMAT_EXTENSIBLE && wf.cbSize >= 22 {
        let ext: WAVEFORMATEXTENSIBLE = unsafe { std::ptr::read_unaligned(p as *const WAVEFORMATEXTENSIBLE) };
        let sub = ext.SubFormat;
        (
            sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
            sub == KSDATAFORMAT_SUBTYPE_PCM,
        )
    } else {
        (tag == WAVE_FORMAT_IEEE_FLOAT, tag == WAVE_FORMAT_PCM)
    };
    let sample = match (float, pcm, bits) {
        (true, _, 32) => Sample::F32,
        (_, true, 16) => Sample::I16,
        (_, true, 24) => Sample::I24,
        (_, true, 32) => Sample::I32,
        _ => return None,
    };
    Some(Fmt {
        rate: wf.nSamplesPerSec,
        channels: wf.nChannels,
        sample,
    })
}

fn make_format(rate: u32, channels: u16, sample: Sample) -> WAVEFORMATEXTENSIBLE {
    let bytes = sample.bytes() as u16;
    let mut f = WAVEFORMATEXTENSIBLE::default();
    f.Format.wFormatTag = WAVE_FORMAT_EXTENSIBLE as u16;
    f.Format.nChannels = channels;
    f.Format.nSamplesPerSec = rate;
    f.Format.wBitsPerSample = bytes * 8;
    f.Format.nBlockAlign = bytes * channels;
    f.Format.nAvgBytesPerSec = rate * (bytes * channels) as u32;
    f.Format.cbSize = 22;
    f.Samples = WAVEFORMATEXTENSIBLE_0 {
        wValidBitsPerSample: if sample == Sample::I32 { 32 } else { bytes * 8 },
    };
    f.dwChannelMask = match channels {
        1 => 0x4, // front centre
        2 => 0x3, // front left + right
        n => (1u32 << n.min(18)) - 1,
    };
    f.SubFormat = if sample == Sample::F32 {
        KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
    } else {
        KSDATAFORMAT_SUBTYPE_PCM
    };
    f
}

/// Device bytes to f32 (interleaved, `frames * channels` samples).
fn read_samples(src: &[u8], fmt: &Fmt, out: &mut [f32]) {
    match fmt.sample {
        Sample::F32 => {
            for (o, b) in out.iter_mut().zip(src.chunks_exact(4)) {
                *o = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
        }
        Sample::I16 => {
            for (o, b) in out.iter_mut().zip(src.chunks_exact(2)) {
                *o = i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0;
            }
        }
        Sample::I24 => {
            for (o, b) in out.iter_mut().zip(src.chunks_exact(3)) {
                *o = (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8388608.0;
            }
        }
        Sample::I32 => {
            for (o, b) in out.iter_mut().zip(src.chunks_exact(4)) {
                *o = i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2147483648.0;
            }
        }
    }
}

/// f32 to device bytes.
fn write_samples(src: &[f32], fmt: &Fmt, dst: &mut [u8]) {
    match fmt.sample {
        Sample::F32 => {
            for (s, d) in src.iter().zip(dst.chunks_exact_mut(4)) {
                d.copy_from_slice(&s.to_le_bytes());
            }
        }
        Sample::I16 => {
            for (s, d) in src.iter().zip(dst.chunks_exact_mut(2)) {
                d.copy_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
            }
        }
        Sample::I24 => {
            for (s, d) in src.iter().zip(dst.chunks_exact_mut(3)) {
                let v = (s.clamp(-1.0, 1.0) * 8388607.0) as i32;
                d.copy_from_slice(&v.to_le_bytes()[..3]);
            }
        }
        Sample::I32 => {
            for (s, d) in src.iter().zip(dst.chunks_exact_mut(4)) {
                d.copy_from_slice(&((s.clamp(-1.0, 1.0) as f64 * 2147483647.0) as i32).to_le_bytes());
            }
        }
    }
}

/// QueryPerformanceCounter in 100 ns units (the unit WASAPI's QPC positions use).
fn qpc_100ns() -> u64 {
    static FREQ: OnceLock<i64> = OnceLock::new();
    let f = *FREQ.get_or_init(|| {
        let mut f = 0i64;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut f);
        }
        f.max(1)
    });
    let mut c = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
    }
    (c as u128 * 10_000_000 / f as u128) as u64
}

// --- enumeration --------------------------------------------------------------------------------------------
fn hardware(store: &IPropertyStore) -> Hardware {
    let path = prop_string(store, &PKEY_INSTANCE_PATH).unwrap_or_default();
    let path = path.strip_prefix("{1}.").unwrap_or(&path).to_string();
    let enumer = prop_string(store, &PKEY_ENUMERATOR)
        .unwrap_or_default()
        .to_uppercase();
    let usb = parse_vid_pid(&path);
    let bus = if enumer == "USB" || enumer.contains("USBAUDIO") || usb.is_some() {
        Bus::Usb
    } else if enumer.starts_with("BTH") || path.to_uppercase().starts_with("BTH") {
        Bus::Bluetooth
    } else if enumer == "HDAUDIO" || enumer == "PCI" || enumer == "INTELAUDIO" {
        Bus::Pci
    } else if enumer == "ROOT" || enumer == "SW" || enumer == "SWD" {
        Bus::Virtual
    } else {
        Bus::Unknown
    };
    Hardware {
        usb,
        instance_paths: if path.is_empty() { vec![] } else { vec![path] },
        adapter_name: prop_string(store, &PKEY_ADAPTER_NAME).unwrap_or_default(),
        bus,
    }
}

/// "…VID_12BA&PID_00FF…" → (0x12BA, 0x00FF)
pub fn parse_vid_pid(path: &str) -> Option<(u16, u16)> {
    let up = path.to_uppercase();
    let hex4 = |s: &str| u16::from_str_radix(s.get(..4)?, 16).ok();
    let v = up.find("VID_")?;
    let p = up.find("PID_")?;
    Some((hex4(&up[v + 4..])?, hex4(&up[p + 4..])?))
}

pub fn enumerate() -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    let Ok(en) = enumerator() else { return out };
    unsafe {
        let default_id = |flow| {
            en.GetDefaultAudioEndpoint(flow, eConsole)
                .ok()
                .and_then(|d| d.GetId().ok())
                .map(pwstr)
        };
        let def_out = default_id(eRender);
        let def_in = default_id(eCapture);
        for flow in [eCapture, eRender] {
            let Ok(col) = en.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else {
                continue;
            };
            for i in 0..col.GetCount().unwrap_or(0) {
                let Ok(dev) = col.Item(i) else { continue };
                let id = dev.GetId().map(pwstr).unwrap_or_default();
                let Ok(store) = dev.OpenPropertyStore(STGM_READ) else {
                    continue;
                };
                let name = prop_string(&store, &PKEY_FRIENDLY_NAME).unwrap_or_else(|| id.clone());
                let (mut channels, mut rate) = (2u16, 48000u32);
                if let Ok(client) = dev.Activate::<IAudioClient>(CLSCTX_ALL, None)
                    && let Ok(mix) = client.GetMixFormat()
                {
                    if let Some(f) = parse_format(mix) {
                        channels = f.channels;
                        rate = f.rate;
                    }
                    CoTaskMemFree(Some(mix as _));
                }
                let input = flow == eCapture;
                let mut hw = hardware(&store);
                if hw.adapter_name.is_empty() {
                    hw.adapter_name = prop_string(&store, &PKEY_DEVICE_DESC).unwrap_or_default();
                }
                out.push(DeviceInfo {
                    id: DeviceId {
                        backend: Backend::Wasapi,
                        id: id.clone(),
                    },
                    name,
                    input_channels: if input { channels } else { 0 },
                    output_channels: if input { 0 } else { channels },
                    input_channel_names: vec![],
                    default_rate: rate,
                    is_default_input: input && def_in.as_deref() == Some(id.as_str()),
                    is_default_output: !input && def_out.as_deref() == Some(id.as_str()),
                    hardware: hw,
                });
            }
        }
    }
    out
}

/// The endpoint description ("Line", "Microphone") of a device, for ranking.
pub fn description(id: &str) -> Option<String> {
    let en = enumerator().ok()?;
    unsafe {
        let dev = en.GetDevice(&HSTRING::from(id)).ok()?;
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        prop_string(&store, &PKEY_DEVICE_DESC)
    }
}

// --- streams ------------------------------------------------------------------------------------------------
struct WasapiStream {
    info: StreamInfo,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    failure: Failure,
}

impl Stream for WasapiStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }
    fn failure(&self) -> Option<String> {
        self.failure.get()
    }
}

impl Drop for WasapiStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Client {
    client: IAudioClient,
    event: HANDLE,
    fmt: Fmt,
    exclusive: bool,
    /// Frames per wake-up.
    period: u32,
    buffer: u32,
    latency: f64,
    name: String,
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.event);
        }
    }
}

fn mmcss() -> Option<HANDLE> {
    let mut idx = 0u32;
    unsafe { AvSetMmThreadCharacteristicsW(windows::core::w!("Pro Audio"), &mut idx).ok() }
}

fn err_text(e: &windows::core::Error) -> String {
    if e.code() == AUDCLNT_E_DEVICE_INVALIDATED {
        "the device was removed".into()
    } else if e.code() == AUDCLNT_E_DEVICE_IN_USE {
        "the device is in use by another program (exclusive mode)".into()
    } else {
        format!("{e}")
    }
}

/// Set up a client on the calling thread: the device's own format in shared mode (smallest period), or a
/// negotiated format in exclusive mode (falling back to shared if the device refuses).
fn setup(
    id: &str,
    render: bool,
    exclusive: bool,
    buffer: BufferSize,
    want_rate: Option<u32>,
) -> Result<Client, String> {
    let en = enumerator().map_err(|e| err_text(&e))?;
    unsafe {
        let dev = en
            .GetDevice(&HSTRING::from(id))
            .map_err(|e| format!("device not found ({})", err_text(&e)))?;
        let name = dev
            .OpenPropertyStore(STGM_READ)
            .ok()
            .and_then(|s| prop_string(&s, &PKEY_FRIENDLY_NAME))
            .unwrap_or_else(|| id.to_string());
        let mut client: IAudioClient = dev.Activate(CLSCTX_ALL, None).map_err(|e| err_text(&e))?;
        let mix = client.GetMixFormat().map_err(|e| err_text(&e))?;
        let mixf = parse_format(mix);
        let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
        let mut chosen: Option<(Fmt, bool, u32)> = None; // (format, exclusive, period frames)

        if exclusive {
            let channels = mixf.map(|f| f.channels).unwrap_or(2);
            let mut rates = vec![];
            for r in [
                want_rate,
                mixf.map(|f| f.rate),
                Some(48000),
                Some(44100),
                Some(96000),
            ]
            .into_iter()
            .flatten()
            {
                if !rates.contains(&r) {
                    rates.push(r);
                }
            }
            'search: for rate in rates {
                for sample in [Sample::F32, Sample::I32, Sample::I24, Sample::I16] {
                    let f = make_format(rate, channels, sample);
                    let p = &f as *const WAVEFORMATEXTENSIBLE as *const WAVEFORMATEX;
                    if client
                        .IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, p, None)
                        .is_ok()
                    {
                        let (mut def, mut min) = (0i64, 0i64);
                        let _ = client.GetDevicePeriod(Some(&mut def), Some(&mut min));
                        let mut period = match buffer {
                            BufferSize::Frames(n) => ((n as f64 * 1e7 / rate as f64) as i64).max(min),
                            BufferSize::Default => min.max(1),
                        };
                        let mut r =
                            client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, period, period, p, None);
                        if let Err(e) = &r
                            && e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED
                        {
                            let frames = client.GetBufferSize().unwrap_or(0);
                            period = (1e7 * frames as f64 / rate as f64 + 0.5) as i64;
                            client = dev.Activate(CLSCTX_ALL, None).map_err(|e| err_text(&e))?;
                            r = client.Initialize(
                                AUDCLNT_SHAREMODE_EXCLUSIVE,
                                flags,
                                period,
                                period,
                                p,
                                None,
                            );
                        }
                        match r {
                            Ok(()) => {
                                let frames = client.GetBufferSize().unwrap_or(0);
                                chosen = Some((
                                    Fmt {
                                        rate,
                                        channels,
                                        sample,
                                    },
                                    true,
                                    frames,
                                ));
                                break 'search;
                            }
                            Err(_) => {
                                // a failed Initialize leaves the client unusable
                                client = dev.Activate(CLSCTX_ALL, None).map_err(|e| err_text(&e))?;
                                break 'search;
                            }
                        }
                    }
                }
            }
        }

        if chosen.is_none() {
            let Some(mf) = mixf else {
                CoTaskMemFree(Some(mix as _));
                return Err("the device's format isn't supported".into());
            };
            let mut period = 0u32;
            if let Ok(c3) = client.cast::<IAudioClient3>() {
                let (mut def, mut fund, mut min, mut max) = (0u32, 0u32, 0u32, 0u32);
                if c3
                    .GetSharedModeEnginePeriod(mix, &mut def, &mut fund, &mut min, &mut max)
                    .is_ok()
                    && fund > 0
                {
                    let want = match buffer {
                        BufferSize::Frames(n) => (n.div_ceil(fund) * fund).clamp(min, max),
                        BufferSize::Default => min,
                    };
                    if c3.InitializeSharedAudioStream(flags, want, mix, None).is_ok() {
                        period = want;
                    }
                }
            }
            if period == 0 {
                client
                    .Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 0, 0, mix, None)
                    .map_err(|e| err_text(&e))?;
                let mut def = 0i64;
                let _ = client.GetDevicePeriod(Some(&mut def), None);
                period = ((def as f64) * mf.rate as f64 / 1e7).round() as u32;
            }
            chosen = Some((mf, false, period.max(16)));
        }
        CoTaskMemFree(Some(mix as _));
        let (fmt, excl, period) = chosen.unwrap();
        let event = CreateEventW(None, false, false, None).map_err(|e| err_text(&e))?;
        client.SetEventHandle(event).map_err(|e| err_text(&e))?;
        let buffer = client.GetBufferSize().map_err(|e| err_text(&e))?;
        let latency = client.GetStreamLatency().unwrap_or(0) as f64 * 1e-7;
        let _ = render;
        Ok(Client {
            client,
            event,
            fmt,
            exclusive: excl,
            period: if excl { buffer } else { period },
            buffer,
            latency,
            name,
        })
    }
}

fn info_of(id: &str, c: &Client) -> StreamInfo {
    StreamInfo {
        device: DeviceId {
            backend: Backend::Wasapi,
            id: id.to_string(),
        },
        device_name: c.name.clone(),
        rate: c.fmt.rate,
        channels: c.fmt.channels,
        block: c.period as usize,
        max_block: c.buffer as usize,
        latency: 0.0,
        mode: format!(
            "WASAPI {} ({})",
            if c.exclusive { "exclusive" } else { "shared" },
            c.fmt.describe()
        ),
    }
}

/// Frames queued beyond one period in shared output (RSAUDIO_OUT_MARGIN overrides; 96 measured clean).
fn out_margin() -> u32 {
    std::env::var("QRS_OUT_MARGIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(96)
}

fn spawn_stream<S: Send + 'static>(
    name: &str,
    id: String,
    render: bool,
    exclusive: bool,
    buffer: BufferSize,
    rate: Option<u32>,
    run: fn(Client, S, &AtomicBool, &Failure),
    make: &mut dyn FnMut(&StreamInfo) -> S,
) -> Result<Box<dyn Stream>, OpenError> {
    let (info_tx, info_rx): (Sender<Result<StreamInfo, String>>, Receiver<_>) = bounded(1);
    let (side_tx, side_rx): (Sender<S>, Receiver<S>) = bounded(1);
    let stop = Arc::new(AtomicBool::new(false));
    let failure = Failure::default();
    let (stop2, fail2, id2) = (stop.clone(), failure.clone(), id.clone());
    let thread = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            com_init();
            let client = match setup(&id2, render, exclusive, buffer, rate) {
                Ok(c) => c,
                Err(e) => {
                    let _ = info_tx.send(Err(e));
                    return;
                }
            };
            let _ = info_tx.send(Ok(info_of(&id2, &client)));
            let Ok(side) = side_rx.recv() else { return };
            let task = mmcss();
            run(client, side, &stop2, &fail2);
            if let Some(t) = task {
                unsafe {
                    let _ = AvRevertMmThreadCharacteristics(t);
                }
            }
        })
        .map_err(oerr)?;
    let info = match info_rx.recv() {
        Ok(Ok(i)) => i,
        Ok(Err(e)) => {
            let _ = thread.join();
            return Err(oerr(e));
        }
        Err(_) => {
            let _ = thread.join();
            return Err(oerr("the audio thread ended during setup"));
        }
    };
    let side = make(&info);
    if side_tx.send(side).is_err() {
        return Err(oerr("the audio thread ended during setup"));
    }
    Ok(Box::new(WasapiStream {
        info,
        stop,
        thread: Some(thread),
        failure,
    }))
}

pub fn open_output(req: &OutputRequest, make: MakeOutput) -> Result<Box<dyn Stream>, OpenError> {
    spawn_stream(
        "qrs-wasapi-out",
        req.device.id.clone(),
        true,
        req.exclusive,
        req.buffer,
        req.rate,
        run_render,
        make,
    )
}

pub fn open_input(req: &InputRequest, make: MakeInput) -> Result<Box<dyn Stream>, OpenError> {
    spawn_stream(
        "qrs-wasapi-in",
        req.device.id.clone(),
        false,
        req.exclusive,
        req.buffer,
        req.rate,
        run_capture,
        make,
    )
}

/// How long without a wake-up before a stream counts as dead (unplugged devices sometimes just go quiet).
const DEAD_AFTER: Duration = Duration::from_secs(3);

fn run_render(c: Client, mut side: OutputSide, stop: &AtomicBool, failure: &Failure) {
    unsafe {
        let render: IAudioRenderClient = match c.client.GetService() {
            Ok(r) => r,
            Err(e) => return failure.fail(err_text(&e)),
        };
        let ch = c.fmt.channels as usize;
        let fb = c.fmt.frame_bytes();
        let rate = c.fmt.rate as f64;
        let mut scratch = vec![0.0f32; c.buffer as usize * ch];
        if c.exclusive {
            // start with one buffer of silence
            if let Ok(p) = render.GetBuffer(c.buffer) {
                std::ptr::write_bytes(p, 0, c.buffer as usize * fb);
                let _ = render.ReleaseBuffer(c.buffer, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32);
            }
        }
        if let Err(e) = c.client.Start() {
            return failure.fail(err_text(&e));
        }
        let target = (c.period + out_margin()).min(c.buffer);
        let mut last_wake = std::time::Instant::now();
        let mut primed = false;
        while !stop.load(Ordering::Acquire) {
            if WaitForSingleObject(c.event, 200) != WAIT_OBJECT_0 {
                if last_wake.elapsed() > DEAD_AFTER {
                    return failure.fail("the output device stopped responding");
                }
                continue;
            }
            last_wake = std::time::Instant::now();
            let (n, padding) = if c.exclusive {
                (c.buffer, 0)
            } else {
                let padding = match c.client.GetCurrentPadding() {
                    Ok(p) => p,
                    Err(e) => return failure.fail(err_text(&e)),
                };
                if padding >= target {
                    continue;
                }
                if padding == 0 && primed {
                    side.note_dropout();
                }
                (target - padding, padding)
            };
            primed = true;
            let p = match render.GetBuffer(n) {
                Ok(p) => p,
                Err(e) => return failure.fail(err_text(&e)),
            };
            let ahead = if c.exclusive {
                c.buffer as f64 / rate
            } else {
                padding as f64 / rate
            };
            let play_ns = now_ns() + ((ahead + c.latency) * 1e9) as u64;
            let buf = &mut scratch[..n as usize * ch];
            side.process(buf, ch, play_ns);
            let dst = std::slice::from_raw_parts_mut(p, n as usize * fb);
            write_samples(buf, &c.fmt, dst);
            if let Err(e) = render.ReleaseBuffer(n, 0) {
                return failure.fail(err_text(&e));
            }
        }
    }
}

fn run_capture(c: Client, mut side: InputSide, stop: &AtomicBool, failure: &Failure) {
    unsafe {
        let capture: IAudioCaptureClient = match c.client.GetService() {
            Ok(r) => r,
            Err(e) => return failure.fail(err_text(&e)),
        };
        let ch = c.fmt.channels as usize;
        let fb = c.fmt.frame_bytes();
        let rate = c.fmt.rate as f64;
        let mut scratch = vec![0.0f32; (c.buffer as usize).max(4096) * ch];
        if let Err(e) = c.client.Start() {
            return failure.fail(err_text(&e));
        }
        let mut last_wake = std::time::Instant::now();
        while !stop.load(Ordering::Acquire) {
            if WaitForSingleObject(c.event, 200) != WAIT_OBJECT_0 {
                if last_wake.elapsed() > DEAD_AFTER {
                    return failure.fail("the input device stopped responding");
                }
                continue;
            }
            last_wake = std::time::Instant::now();
            loop {
                let next = match capture.GetNextPacketSize() {
                    Ok(n) => n,
                    Err(e) => return failure.fail(err_text(&e)),
                };
                if next == 0 {
                    break;
                }
                let (mut data, mut frames, mut flags, mut qpc) = (std::ptr::null_mut(), 0u32, 0u32, 0u64);
                if let Err(e) = capture.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc)) {
                    return failure.fail(err_text(&e));
                }
                let n = (frames as usize).min(scratch.len() / ch);
                let buf = &mut scratch[..n * ch];
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                    buf.fill(0.0);
                } else {
                    read_samples(std::slice::from_raw_parts(data, n * fb), &c.fmt, buf);
                }
                if flags & (AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32) != 0 {
                    side.note_dropout();
                }
                // WASAPI's QPC time of the first frame, mapped onto our clock
                let now = now_ns();
                let capture_ns = if qpc > 0 {
                    now.saturating_sub(qpc_100ns().saturating_sub(qpc) * 100)
                } else {
                    now.saturating_sub((n as f64 / rate * 1e9) as u64)
                };
                side.process(buf, ch, capture_ns);
                if let Err(e) = capture.ReleaseBuffer(frames) {
                    return failure.fail(err_text(&e));
                }
            }
        }
    }
}

// --- hot-plug -----------------------------------------------------------------------------------------------
#[implement(IMMNotificationClient)]
struct Notifier {
    tx: Sender<DeviceEvent>,
}

fn pcwstr(p: &PCWSTR) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { p.to_string().unwrap_or_default() }
    }
}

impl IMMNotificationClient_Impl for Notifier_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: DEVICE_STATE) -> windows::core::Result<()> {
        let id = DeviceId {
            backend: Backend::Wasapi,
            id: pcwstr(id),
        };
        let _ = self.tx.send(DeviceEvent::State(id, state == DEVICE_STATE_ACTIVE));
        Ok(())
    }
    fn OnDeviceAdded(&self, id: &PCWSTR) -> windows::core::Result<()> {
        let _ = self.tx.send(DeviceEvent::Added(DeviceId {
            backend: Backend::Wasapi,
            id: pcwstr(id),
        }));
        Ok(())
    }
    fn OnDeviceRemoved(&self, id: &PCWSTR) -> windows::core::Result<()> {
        let _ = self.tx.send(DeviceEvent::Removed(DeviceId {
            backend: Backend::Wasapi,
            id: pcwstr(id),
        }));
        Ok(())
    }
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _id: &PCWSTR,
    ) -> windows::core::Result<()> {
        if role == eConsole {
            let _ = self.tx.send(DeviceEvent::DefaultChanged {
                input: flow == eCapture,
            });
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Device arrivals, removals and default-device changes, for as long as this lives (keep it on one thread).
pub struct Watcher {
    en: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        unsafe {
            let _ = self.en.UnregisterEndpointNotificationCallback(&self.client);
        }
    }
}

pub fn watch(tx: Sender<DeviceEvent>) -> Option<Watcher> {
    let en = enumerator().ok()?;
    let client: IMMNotificationClient = Notifier { tx }.into();
    unsafe { en.RegisterEndpointNotificationCallback(&client).ok()? };
    Some(Watcher { en, client })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vid_pid() {
        assert_eq!(
            parse_vid_pid(r"USB\VID_12BA&PID_00FF&MI_00\7&1"),
            Some((0x12BA, 0x00FF))
        );
        assert_eq!(
            parse_vid_pid(r"TUSBAUDIO_ENUM\VID_1FC9&PID_8260&KS\2412"),
            Some((0x1FC9, 0x8260))
        );
        assert_eq!(parse_vid_pid(r"HDAUDIO\FUNC_01&VEN_10EC"), None);
    }

    #[test]
    fn sample_conversions_roundtrip() {
        for sample in [Sample::F32, Sample::I16, Sample::I24, Sample::I32] {
            let fmt = Fmt {
                rate: 48000,
                channels: 2,
                sample,
            };
            let src = [0.5f32, -0.25, 0.0, 0.999];
            let mut bytes = vec![0u8; src.len() * sample.bytes()];
            write_samples(&src, &fmt, &mut bytes);
            let mut back = [0.0f32; 4];
            read_samples(&bytes, &fmt, &mut back);
            for (a, b) in src.iter().zip(back) {
                assert!((a - b).abs() < 1e-4, "{sample:?} {a} {b}");
            }
        }
    }

    #[test]
    fn extensible_format_parses() {
        let f = make_format(44100, 1, Sample::I24);
        let p = unsafe { parse_format(&f as *const WAVEFORMATEXTENSIBLE as *const WAVEFORMATEX) }.unwrap();
        assert_eq!((p.rate, p.channels, p.sample), (44100, 1, Sample::I24));
    }
}
