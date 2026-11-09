//! ASIO (Windows) through cpal's ASIO host: an audio interface's own driver (as RS_ASIO uses it), in duplex (one
//! callback for input and output) or input-only, with the driver's or a chosen buffer size. One ASIO driver can
//! be open per process.

use super::cpal_host::{self, Open, Side};
use super::{
    Backend, DeviceInfo, InputRequest, MakeDuplex, MakeInput, MakeOutput, OpenError, OutputRequest, Stream,
    oerr,
};

const HOST: Option<cpal::HostId> = Some(cpal::HostId::Asio);

/// The ASIO drivers' devices. Listing them loads each driver into the process, and a driver that crashes when it's
/// loaded (issue #1: a Line 6 HX Stomp's, whichever device was chosen) takes the process with it. So once
/// `helper()` has been called (the programs do, first thing), each driver is first loaded by a helper process, this
/// program run again with `--asio-probe`, and this process sees and loads only those that loaded there (the patched
/// asio-sys, D186). A driver that crashes or hangs the helper is left out until quickRS restarts (`left_out`).
/// Without the helper (tests, examples) the drivers are loaded here, as before.
pub fn enumerate() -> Vec<DeviceInfo> {
    match HELPER.get() {
        Some(exe) => probe::enumerate(exe),
        None => enumerate_here(),
    }
}

/// The drivers loaded in this process. The ASIO SDK's driver list initialises COM itself and finds nothing on a
/// thread in the multithreaded apartment unless that thread was the process's first ASIO user (measured with
/// examples/apartment_probe.rs: the device manager's thread lost the NUX driver 5 s after start). So drivers
/// are listed from a thread of their own, the way streams are opened (cpal_host::open).
fn enumerate_here() -> Vec<DeviceInfo> {
    std::thread::scope(|s| {
        s.spawn(|| cpal_host::enumerate(HOST, Backend::Asio, false))
            .join()
            .unwrap_or_default()
    })
}

/// This program, for loading ASIO drivers in a helper process (set by `helper`).
static HELPER: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Call first thing in `main`. Run as the helper (`--asio-probe DRIVER...`), this loads each driver named, alone,
/// reports what it found on stdout and exits; otherwise it switches the helper on for this process, and until the
/// helper has loaded a driver safely this process loads none.
pub fn helper() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(probe::FLAG) {
        probe::serve(&args[1..]);
        std::process::exit(0);
    }
    if let Ok(exe) = std::env::current_exe() {
        asio_sys::show_only_drivers(Some(Vec::new()));
        let _ = HELPER.set(exe);
    }
}

/// The drivers left out because they crashed or hung the helper (shown in Settings > Audio).
pub fn left_out() -> Vec<String> {
    probe::LEFT_OUT.lock().clone()
}

mod probe {
    use std::io::{BufRead, BufReader, Write};
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use parking_lot::Mutex;

    use super::super::DeviceInfo;

    pub const FLAG: &str = "--asio-probe";
    /// How long one driver may take to load before it counts as hung.
    const PATIENCE: Duration = Duration::from_secs(10);

    pub static LEFT_OUT: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// One line of the helper's report.
    #[derive(Debug, PartialEq)]
    pub enum Line {
        /// About to load this driver.
        Trying(String),
        /// It loaded: its devices (none when its interface isn't there).
        Found(String, Vec<DeviceInfo>),
    }

    pub fn format(l: &Line) -> String {
        match l {
            Line::Trying(n) => format!("try\t{n}"),
            Line::Found(n, d) => format!(
                "found\t{n}\t{}",
                serde_json::to_string(d).unwrap_or_else(|_| "[]".into())
            ),
        }
    }

    pub fn parse(s: &str) -> Option<Line> {
        let mut it = s.trim_end_matches(['\r', '\n']).splitn(3, '\t');
        match (it.next()?, it.next()?, it.next()) {
            ("try", n, None) => Some(Line::Trying(n.into())),
            ("found", n, Some(j)) => Some(Line::Found(n.into(), serde_json::from_str(j).ok()?)),
            _ => None,
        }
    }

    /// The helper: each driver named, loaded alone, in turn.
    pub fn serve(names: &[String]) {
        let mut out = std::io::stdout().lock();
        for n in names {
            let _ = writeln!(out, "{}", format(&Line::Trying(n.clone())));
            let _ = out.flush();
            // (a test: QUICKRS_ASIO_CRASH=<driver> makes the helper crash on that driver, as the HX Stomp's does)
            if std::env::var("QUICKRS_ASIO_CRASH").is_ok_and(|c| c == *n) {
                std::process::abort();
            }
            asio_sys::show_only_drivers(Some(vec![n.clone()]));
            let devices = super::enumerate_here();
            let _ = writeln!(out, "{}", format(&Line::Found(n.clone(), devices)));
            let _ = out.flush();
        }
    }

    /// What the helper made of the drivers to try: their devices, the drivers that loaded, and the one it was
    /// loading when it stopped (crashed or hung), if it stopped early.
    pub fn read(lines: impl IntoIterator<Item = Line>) -> (Vec<DeviceInfo>, Vec<String>, Option<String>) {
        let (mut devices, mut loaded, mut trying) = (Vec::new(), Vec::new(), None);
        for l in lines {
            match l {
                Line::Trying(n) => trying = Some(n),
                Line::Found(n, d) => {
                    trying = None;
                    devices.extend(d);
                    loaded.push(n);
                }
            }
        }
        (devices, loaded, trying)
    }

    /// Run the helper on one driver; its report's lines until it ends or goes quiet for `PATIENCE` (then it's stopped).
    fn run(exe: &Path, name: &str) -> Option<Vec<Line>> {
        let mut cmd = Command::new(exe);
        cmd.arg(FLAG)
            .arg(name)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().ok()?;
        let stdout = child.stdout.take()?;
        let (tx, rx) = crossbeam_channel::unbounded();
        let reader = std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines() {
                let Ok(l) = l else { break };
                if let Some(l) = parse(&l) {
                    let _ = tx.send(l);
                }
            }
        });
        let mut lines = Vec::new();
        loop {
            match rx.recv_timeout(PATIENCE) {
                Ok(l) => lines.push(l),
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    let _ = child.kill();
                    break;
                }
            }
        }
        let _ = child.wait();
        let _ = reader.join();
        Some(lines)
    }

    /// Every driver's devices, each driver loaded by the helper first; this process may then load those that loaded.
    pub fn enumerate(exe: &Path) -> Vec<DeviceInfo> {
        let devices = enumerate_with(super::driver_names(), &LEFT_OUT, |name| run(exe, name));
        // this process loads only drivers with a device that loaded in the helper
        // (by the device's name, which is asio-sys's name for the driver: it cuts a long registry name short)
        let shown: Vec<String> = devices.iter().map(|d: &DeviceInfo| d.id.id.clone()).collect();
        asio_sys::show_only_drivers(Some(shown));
        devices
    }

    /// The helper (`run`) on each driver not yet left out, a fresh helper for each: in one process, a driver that
    /// fails to start (Focusrite's Thunderbolt driver without its interface) leaves the ASIO SDK so that the next
    /// driver loaded crashes it (measured on the owner's PC).
    pub fn enumerate_with(
        names: Vec<String>,
        left: &Mutex<Vec<String>>,
        mut run: impl FnMut(&str) -> Option<Vec<Line>>,
    ) -> Vec<DeviceInfo> {
        let left_out = left.lock().clone();
        let mut devices = Vec::new();
        for name in names.into_iter().filter(|n| !left_out.contains(n)) {
            let Some(lines) = run(&name) else {
                log::warn!("ASIO: the helper wouldn't start; ASIO drivers left out");
                break;
            };
            let (d, _, stopped) = read(lines);
            devices.extend(d);
            if let Some(bad) = stopped {
                log::warn!(
                    "ASIO: the driver \"{bad}\" crashed or hung when loaded; left out until quickRS restarts"
                );
                left.lock().push(bad);
            }
        }
        devices
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::backend::{Backend, DeviceId, Hardware};

        fn dev(name: &str) -> DeviceInfo {
            DeviceInfo {
                id: DeviceId {
                    backend: Backend::Asio,
                    id: name.into(),
                },
                name: name.into(),
                input_channels: 2,
                output_channels: 2,
                input_channel_names: vec![],
                default_rate: 48000,
                is_default_input: false,
                is_default_output: false,
                hardware: Hardware::default(),
            }
        }

        #[test]
        fn the_helpers_report_reads_back() {
            for l in [
                Line::Trying("Line 6 ASIO".into()),
                Line::Found("NUX Audio".into(), vec![dev("NUX Audio")]),
                Line::Found("Realtek ASIO".into(), vec![]),
            ] {
                assert_eq!(parse(&format(&l)), Some(l));
            }
            assert_eq!(parse("ASIO SDK says hello"), None);
        }

        /// A helper that crashes on "Bad" and hangs on "Hang" (its report stops at "try"), loads the rest.
        fn helper(runs: &mut Vec<String>) -> impl FnMut(&str) -> Option<Vec<Line>> + '_ {
            move |n| {
                runs.push(n.into());
                let mut out = vec![Line::Trying(n.into())];
                if n != "Bad" && n != "Hang" {
                    let d = if n == "Off" { vec![] } else { vec![dev(n)] };
                    out.push(Line::Found(n.into(), d));
                }
                Some(out)
            }
        }

        #[test]
        fn a_driver_that_crashes_the_helper_is_left_out_and_the_rest_still_load() {
            let left = Mutex::new(Vec::new());
            let mut runs = Vec::new();
            let names = ["A", "Bad", "Off", "Hang", "C"].map(String::from).to_vec();
            let d = enumerate_with(names.clone(), &left, helper(&mut runs));
            let got: Vec<&str> = d.iter().map(|d| d.name.as_str()).collect();
            assert_eq!(got, ["A", "C"]);
            assert_eq!(*left.lock(), ["Bad", "Hang"]);
            assert_eq!(runs, names);
            // the next scan doesn't try them again
            let mut runs = Vec::new();
            let d = enumerate_with(names, &left, helper(&mut runs));
            assert_eq!(d.len(), 2);
            assert_eq!(runs, ["A", "Off", "C"]);
        }

        #[test]
        fn no_helper_means_no_asio_drivers_rather_than_loading_them_here() {
            let left = Mutex::new(Vec::new());
            let d = enumerate_with(vec!["A".into()], &left, |_| None);
            assert!(d.is_empty());
        }
    }
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
