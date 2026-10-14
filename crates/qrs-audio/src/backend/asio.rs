//! ASIO (Windows) through cpal's ASIO host: an audio interface's own driver (as RS_ASIO uses it), in duplex (one
//! callback for input and output) or input-only, with the driver's or a chosen buffer size. One ASIO driver can
//! be open per process.

use super::cpal_host::{self, Open, Side};
use super::{
    Backend, DeviceInfo, InputRequest, MakeDuplex, MakeInput, MakeOutput, OpenError, OutputRequest, Stream,
    oerr,
};

const HOST: Option<cpal::HostId> = Some(cpal::HostId::Asio);

/// The ASIO drivers. The ASIO SDK's driver list initialises COM itself and finds nothing on a thread in the
/// multithreaded apartment unless that thread was the process's first ASIO user (measured with
/// examples/apartment_probe.rs: the device manager's thread lost the NUX driver 5 s after start). So drivers
/// are listed from a thread of their own, the way streams are opened (cpal_host::open).
pub fn enumerate() -> Vec<DeviceInfo> {
    std::thread::scope(|s| {
        s.spawn(|| cpal_host::enumerate(HOST, Backend::Asio, false))
            .join()
            .unwrap_or_default()
    })
}

/// The ASIO drivers installed, from the registry (HKLM\SOFTWARE\ASIO): nothing is loaded, so it's safe to ask
/// often (listing them with `enumerate` loads each driver, which can disturb an interface in use over WASAPI).
pub fn driver_names() -> Vec<String> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW,
    };
    use windows::core::{PWSTR, w};
    let mut out = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\ASIO"),
            Some(0),
            KEY_READ,
            &mut key,
        ) != ERROR_SUCCESS
        {
            return out;
        }
        for i in 0..256u32 {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            if RegEnumKeyExW(
                key,
                i,
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            ) != ERROR_SUCCESS
            {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        let _ = RegCloseKey(key);
    }
    out.sort();
    out
}

fn base(id: &str, input: bool, output: bool, buffer: super::BufferSize, rate: Option<u32>) -> Open {
    Open {
        host: HOST,
        backend: Backend::Asio,
        mode: "ASIO",
        id: id.into(),
        input,
        output,
        buffer,
        rate,
        out_channels: 2,
    }
}

pub fn open_input(req: &InputRequest, make: MakeInput) -> Result<Box<dyn Stream>, OpenError> {
    cpal_host::open(
        base(&req.device.id, true, false, req.buffer, req.rate),
        &mut |i, _| Side::Input(make(i.unwrap())),
    )
}

pub fn open_output(req: &OutputRequest, make: MakeOutput) -> Result<Box<dyn Stream>, OpenError> {
    cpal_host::open(
        base(&req.device.id, false, true, req.buffer, req.rate),
        &mut |_, o| Side::Output(make(o.unwrap())),
    )
}

pub fn open_duplex(
    inp: &InputRequest,
    out: &OutputRequest,
    make: MakeDuplex,
) -> Result<Box<dyn Stream>, OpenError> {
    if inp.device != out.device {
        return Err(oerr("ASIO duplex needs one driver"));
    }
    cpal_host::open(base(&inp.device.id, true, true, out.buffer, None), &mut |i, o| {
        Side::Duplex(make(i.unwrap(), o.unwrap()))
    })
}
