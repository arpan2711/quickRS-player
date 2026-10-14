//! The system's default audio host through cpal: WASAPI shared on Windows (as a fallback to our own WASAPI
//! backend), CoreAudio on a Mac, ALSA/PulseAudio on Linux. "default-in" / "default-out" follow the system default.

use super::cpal_host::{self, Open, Side};
use super::{Backend, DeviceInfo, InputRequest, MakeInput, MakeOutput, OpenError, OutputRequest, Stream};

pub fn enumerate() -> Vec<DeviceInfo> {
    // on Windows our own WASAPI backend lists the devices; here just the "System default" entries
    cpal_host::enumerate(None, Backend::System, cfg!(windows))
}

fn base(id: &str, input: bool, buffer: super::BufferSize, rate: Option<u32>) -> Open {
    Open {
        host: None,
        backend: Backend::System,
        mode: "System",
        id: id.into(),
        input,
        output: !input,
        buffer,
        rate,
        out_channels: 2,
    }
}

pub fn open_input(req: &InputRequest, make: MakeInput) -> Result<Box<dyn Stream>, OpenError> {
    cpal_host::open(base(&req.device.id, true, req.buffer, req.rate), &mut |i, _| {
        Side::Input(make(i.unwrap()))
    })
}

pub fn open_output(req: &OutputRequest, make: MakeOutput) -> Result<Box<dyn Stream>, OpenError> {
    cpal_host::open(base(&req.device.id, false, req.buffer, req.rate), &mut |_, o| {
        Side::Output(make(o.unwrap()))
    })
}
