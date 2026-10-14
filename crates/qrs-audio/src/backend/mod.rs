//! Device APIs behind one interface. Each backend lists its devices and opens streams; a stream is opened in one
//! call that negotiates the format, then asks the engine to build the real-time side for exactly that format
//! (rate, channels, block size), then starts it. Dropping the returned handle stops the stream.
//!
//! Backends: WASAPI (Windows, shared and exclusive, own implementation: device instance ids for the Real Tone
//! Cable, hot-plug notifications), ASIO (Windows, through cpal), System (cpal's default host: the portable path,
//! CoreAudio on a Mac), Null (a timer instead of a sound card: tests, and a silent fallback).

#[cfg(all(windows, feature = "asio"))]
pub mod asio;
pub mod cpal_host;
pub mod null;
pub mod system;
#[cfg(windows)]
pub mod wasapi;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::rt::Duplex;
use crate::rt::input::InputSide;
use crate::rt::output::OutputSide;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Backend {
    Wasapi,
    Asio,
    System,
    Null,
}

impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Backend::Wasapi => "WASAPI",
            Backend::Asio => "ASIO",
            Backend::System => "System",
            Backend::Null => "None",
        }
    }
}

/// A device, stable across runs: the backend's own id (WASAPI endpoint id, ASIO driver name, cpal device name).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId {
    pub backend: Backend,
    pub id: String,
}

/// Device arrivals and removals (WASAPI notifications; other backends are polled).
#[derive(Clone, Debug, PartialEq)]
pub enum DeviceEvent {
    Added(DeviceId),
    Removed(DeviceId),
    /// Became active (true) or unavailable (false).
    State(DeviceId, bool),
    /// The system's default input or output changed.
    DefaultChanged {
        input: bool,
    },
}

/// What we know about the hardware behind a device (for picking the guitar input automatically).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hardware {
    /// USB vendor and product id, when the device is on USB.
    pub usb: Option<(u16, u16)>,
    /// PnP instance paths of the device and its parents (e.g. "USB\VID_12BA&PID_00FF&MI_00\...").
    pub instance_paths: Vec<String>,
    /// The adapter's own name (e.g. "Rocksmith Guitar Adapter"), when different from the endpoint name.
    pub adapter_name: String,
    pub bus: Bus,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bus {
    Usb,
    Pci,
    Bluetooth,
    Virtual,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub input_channels: u16,
    pub output_channels: u16,
    /// Channel names where the driver gives them (ASIO: "IN 1", "IN 2").
    pub input_channel_names: Vec<String>,
    pub default_rate: u32,
    pub is_default_input: bool,
    pub is_default_output: bool,
    pub hardware: Hardware,
}

impl DeviceInfo {
    pub fn label(&self) -> String {
        match self.id.backend {
            Backend::Asio => format!("{} (ASIO)", self.name),
            _ => self.name.clone(),
        }
    }
}

/// How big a buffer to ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BufferSize {
    /// The driver's own setting (ASIO) / the device's minimum period (WASAPI).
    #[default]
    Default,
    Frames(u32),
}

#[derive(Clone, Debug)]
pub struct InputRequest {
    pub device: DeviceId,
    pub buffer: BufferSize,
    pub exclusive: bool,
    /// Preferred rate (the output's), if the device can do it.
    pub rate: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct OutputRequest {
    pub device: DeviceId,
    pub buffer: BufferSize,
    pub exclusive: bool,
    pub rate: Option<u32>,
}

/// The format a stream was opened with.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamInfo {
    pub device: DeviceId,
    pub device_name: String,
    pub rate: u32,
    pub channels: u16,
    /// Typical frames per callback.
    pub block: usize,
    /// Largest block the backend may deliver (for preallocating).
    pub max_block: usize,
    /// Device latency (seconds) not already included in the block timestamps the backend gives.
    pub latency: f64,
    /// "WASAPI shared", "WASAPI exclusive", "ASIO", "System", "None".
    pub mode: String,
}

/// A running stream; dropping it stops the stream.
pub trait Stream: Send {
    fn info(&self) -> &StreamInfo;
    /// Set when the stream died (device unplugged, driver reset); the engine reopens.
    fn failure(&self) -> Option<String>;
}

/// Shared failure flag for streams' error callbacks.
#[derive(Clone, Default)]
pub struct Failure {
    set: Arc<AtomicBool>,
    msg: Arc<Mutex<Option<String>>>,
}

impl Failure {
    pub fn fail(&self, msg: impl Into<String>) {
        if !self.set.swap(true, Ordering::AcqRel) {
            *self.msg.lock() = Some(msg.into());
        }
    }
    pub fn get(&self) -> Option<String> {
        if self.set.load(Ordering::Acquire) {
            self.msg.lock().clone().or_else(|| Some("stream failed".into()))
        } else {
            None
        }
    }
}

pub type MakeInput<'a> = &'a mut dyn FnMut(&StreamInfo) -> InputSide;
pub type MakeOutput<'a> = &'a mut dyn FnMut(&StreamInfo) -> OutputSide;
pub type MakeDuplex<'a> = &'a mut dyn FnMut(&StreamInfo, &StreamInfo) -> Duplex;

#[derive(Debug)]
pub struct OpenError(pub String);

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OpenError {}

pub fn oerr(m: impl std::fmt::Display) -> OpenError {
    OpenError(m.to_string())
}

/// Every device every backend can see.
pub fn enumerate() -> Vec<DeviceInfo> {
    enumerate_except(None)
}

/// Every device, leaving one backend out (the device manager skips ASIO while an ASIO stream runs).
pub fn enumerate_except(skip: Option<Backend>) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    for b in [Backend::Wasapi, Backend::Asio, Backend::System, Backend::Null] {
        if Some(b) != skip {
            out.extend(enumerate_only(b));
        }
    }
    out
}

/// The ASIO drivers installed (names only; nothing is loaded).
pub fn asio_driver_names() -> Vec<String> {
    #[cfg(all(windows, feature = "asio"))]
    {
        asio::driver_names()
    }
    #[cfg(not(all(windows, feature = "asio")))]
    {
        Vec::new()
    }
}

/// One backend's devices.
pub fn enumerate_only(b: Backend) -> Vec<DeviceInfo> {
    match b {
        #[cfg(windows)]
        Backend::Wasapi => wasapi::enumerate(),
        #[cfg(all(windows, feature = "asio"))]
        Backend::Asio => asio::enumerate(),
        Backend::System => system::enumerate(),
        Backend::Null => null::enumerate(),
        #[allow(unreachable_patterns)]
        _ => Vec::new(),
    }
}

pub fn open_input(req: &InputRequest, make: MakeInput) -> Result<Box<dyn Stream>, OpenError> {
    match req.device.backend {
        #[cfg(windows)]
        Backend::Wasapi => wasapi::open_input(req, make),
        #[cfg(all(windows, feature = "asio"))]
        Backend::Asio => asio::open_input(req, make),
        Backend::System => system::open_input(req, make),
        Backend::Null => null::open_input(req, make),
        #[allow(unreachable_patterns)]
        b => Err(oerr(format!("{} isn't available in this build", b.label()))),
    }
}

pub fn open_output(req: &OutputRequest, make: MakeOutput) -> Result<Box<dyn Stream>, OpenError> {
    match req.device.backend {
        #[cfg(windows)]
        Backend::Wasapi => wasapi::open_output(req, make),
        #[cfg(all(windows, feature = "asio"))]
        Backend::Asio => asio::open_output(req, make),
        Backend::System => system::open_output(req, make),
        Backend::Null => null::open_output(req, make),
        #[allow(unreachable_patterns)]
        b => Err(oerr(format!("{} isn't available in this build", b.label()))),
    }
}

/// One device for both directions in one callback (ASIO; Null for tests). Other backends return an error and the
/// engine opens two streams instead.
pub fn open_duplex(
    inp: &InputRequest,
    out: &OutputRequest,
    make: MakeDuplex,
) -> Result<Box<dyn Stream>, OpenError> {
    if inp.device != out.device {
        return Err(oerr("duplex needs the same device for input and output"));
    }
    match inp.device.backend {
        #[cfg(all(windows, feature = "asio"))]
        Backend::Asio => asio::open_duplex(inp, out, make),
        Backend::Null => null::open_duplex(inp, out, make),
        b => Err(oerr(format!("{} has no duplex mode", b.label()))),
    }
}

/// Whether this backend can only run one device at a time (ASIO: one driver per process).
pub fn exclusive_driver(b: Backend) -> bool {
    b == Backend::Asio
}
