//! Which input is the guitar: every device gets a score and a reason, highest first.
//!
//! 1. Rocksmith's Real Tone Cable (USB VID_12BA&PID_00FF, what Rocksmith itself looks for).
//! 2. The ASIO input from the imported RS_ASIO setup.
//! 3. An audio interface's own ASIO driver (generic wrappers like ASIO4ALL rank lower).
//! 4. A USB input that looks like an instrument interface ("Line", "Instrument", "Hi-Z", known brands);
//!    when the same hardware also has an ASIO driver, the ASIO one wins.
//! 5. Other USB / built-in inputs; webcams and headsets are microphones (acoustic guitar mode).
//! 6. The system default input (the portable path).
//!
//! Virtual devices (Steam streaming, VB-Audio, Voicemeeter, Stereo Mix) and the test devices are never picked
//! automatically.

use serde::{Deserialize, Serialize};

use super::rocksmith::RocksmithSetup;
use crate::backend::{Backend, Bus, DeviceInfo};

pub const RTC_VID_PID: (u16, u16) = (0x12BA, 0x00FF);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputKind {
    /// Rocksmith Real Tone Cable.
    RealToneCable,
    /// An audio interface / guitar interface / amp modeller.
    Interface,
    /// A microphone (webcam, headset, built-in mic): for an acoustic guitar.
    Microphone,
    /// The system default input, whatever it is.
    SystemDefault,
    Virtual,
    Test,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankedInput {
    pub device: DeviceInfo,
    pub score: i32,
    pub kind: InputKind,
    pub reason: String,
}

const INTERFACE_BRANDS: [&str; 40] = [
    "nux",
    "focusrite",
    "scarlett",
    "clarett",
    "line 6",
    "pod",
    "helix",
    "boss",
    "roland",
    "zoom",
    "behringer",
    "umc",
    "m-audio",
    "presonus",
    "steinberg",
    "universal audio",
    "volt",
    "irig",
    "ik multimedia",
    "fender",
    "positive grid",
    "spark",
    "audient",
    "evo",
    "motu",
    "tascam",
    "mackie",
    "native instruments",
    "komplete",
    "yamaha",
    "audio-technica",
    "rme",
    "babyface",
    "apogee",
    "art",
    "esi",
    "neural dsp",
    "quad cortex",
    "headrush",
    "katana",
];
const MIC_WORDS: [&str; 9] = [
    "webcam",
    "camera",
    "cam ",
    "headset",
    "hands-free",
    "bluetooth",
    "airpods",
    "conf",
    "array",
];
const VIRTUAL_WORDS: [&str; 10] = [
    "steam streaming",
    "vb-audio",
    "voicemeeter",
    "cable output",
    "stereo mix",
    "what u hear",
    "virtual",
    "obs",
    "nvidia broadcast",
    "krisp",
];
const GENERIC_ASIO: [&str; 5] = [
    "asio4all",
    "flexasio",
    "asio2wasapi",
    "voicemeeter",
    "generic low latency",
];

fn has_any(s: &str, words: &[&str]) -> bool {
    words.iter().any(|w| s.contains(w))
}

/// A device's name as one lowercase string (friendly name + adapter name).
fn text(d: &DeviceInfo) -> String {
    format!("{} {}", d.name, d.hardware.adapter_name).to_lowercase()
}

/// Steam streaming, VB-Audio cables, Voicemeeter, Stereo Mix and the like (never picked by themselves).
pub fn is_virtual(d: &DeviceInfo) -> bool {
    let t = text(d);
    d.hardware.bus == Bus::Virtual || has_any(&t, &VIRTUAL_WORDS) || t.contains("cable input")
}

pub fn is_real_tone_cable(d: &DeviceInfo) -> bool {
    let t = text(d);
    d.hardware.usb == Some(RTC_VID_PID)
        || d.hardware
            .instance_paths
            .iter()
            .any(|p| p.to_uppercase().contains("VID_12BA&PID_00FF"))
        || t.contains("rocksmith")
        || t.contains("real tone")
}

/// Score every input device (devices without inputs are left out).
pub fn rank_inputs(devices: &[DeviceInfo], rs: Option<&RocksmithSetup>) -> Vec<RankedInput> {
    let asio_names: Vec<String> = devices
        .iter()
        .filter(|d| d.id.backend == Backend::Asio)
        .map(|d| d.name.to_lowercase())
        .collect();
    let rs_asio_input = rs
        .and_then(|s| s.rs_asio.as_ref())
        .filter(|a| a.enable_asio)
        .and_then(|a| a.inputs.first())
        .map(|(drv, _)| drv.to_lowercase());
    let mut out: Vec<RankedInput> = devices
        .iter()
        .filter(|d| d.input_channels > 0)
        .map(|d| {
            let t = text(d);
            let (score, kind, reason): (i32, InputKind, String) = match d.id.backend {
                Backend::Null => (-100, InputKind::Test, "test device".into()),
                Backend::System => (100, InputKind::SystemDefault, "the system's default input".into()),
                _ if is_real_tone_cable(d) => {
                    (1000, InputKind::RealToneCable, "Rocksmith Real Tone Cable".into())
                }
                Backend::Asio => {
                    if rs_asio_input.as_deref() == Some(d.name.to_lowercase().as_str()) {
                        (
                            900,
                            InputKind::Interface,
                            "your Rocksmith (RS_ASIO) guitar input".into(),
                        )
                    } else if has_any(&t, &GENERIC_ASIO) {
                        (300, InputKind::Other, "generic ASIO driver".into())
                    } else {
                        (700, InputKind::Interface, "audio interface (ASIO driver)".into())
                    }
                }
                _ if has_any(&t, &VIRTUAL_WORDS) || d.hardware.bus == Bus::Virtual => {
                    (0, InputKind::Virtual, "virtual device".into())
                }
                _ => {
                    let desc_line = t.starts_with("line")
                        || t.contains("instrument")
                        || t.contains("hi-z")
                        || t.contains("guitar")
                        || t.contains(" inst");
                    let brand = has_any(&t, &INTERFACE_BRANDS);
                    let mic = has_any(&t, &MIC_WORDS) || (t.starts_with("microphone") && !brand);
                    let has_asio_twin = asio_names.iter().any(|n| !n.is_empty() && t.contains(n.as_str()));
                    match (d.hardware.bus, desc_line || brand, mic) {
                        (_, true, _) if has_asio_twin => (
                            550,
                            InputKind::Interface,
                            "audio interface (its ASIO driver is preferred)".into(),
                        ),
                        (Bus::Usb, true, _) => (600, InputKind::Interface, "USB audio interface".into()),
                        (_, true, false) => (450, InputKind::Interface, "line input".into()),
                        (Bus::Bluetooth, _, _) => (80, InputKind::Microphone, "Bluetooth microphone".into()),
                        (_, _, true) => (
                            150,
                            InputKind::Microphone,
                            "microphone (for an acoustic guitar)".into(),
                        ),
                        (Bus::Usb, _, _) => (400, InputKind::Other, "USB audio input".into()),
                        (Bus::Pci, _, _) => (200, InputKind::Microphone, "built-in input".into()),
                        _ => (250, InputKind::Other, "audio input".into()),
                    }
                }
            };
            RankedInput {
                device: d.clone(),
                score: score + d.is_default_input as i32,
                kind,
                reason,
            }
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.score));
    out
}

/// The guitar channel the meters suggest: the one clearly louder than the rest while a string rings, or all of
/// them mixed (-1) when they carry the same signal. None while nothing is playing.
pub fn suggest_channel(peaks: &[f32]) -> Option<i32> {
    let (best, &max) = peaks.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    if max < 0.02 {
        return None;
    }
    let others = peaks
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != best)
        .map(|(_, p)| *p)
        .fold(0.0f32, f32::max);
    if others < max * 0.25 {
        Some(best as i32)
    } else {
        Some(-1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{DeviceId, Hardware};

    fn dev(
        backend: Backend,
        name: &str,
        adapter: &str,
        bus: Bus,
        usb: Option<(u16, u16)>,
        path: &str,
    ) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId {
                backend,
                id: name.into(),
            },
            name: name.into(),
            input_channels: 2,
            output_channels: 0,
            input_channel_names: vec![],
            default_rate: 48000,
            is_default_input: false,
            is_default_output: false,
            hardware: Hardware {
                usb,
                instance_paths: vec![path.into()],
                adapter_name: adapter.into(),
                bus,
            },
        }
    }

    /// The devices on the owner's PC (2026-10-14), plus a Real Tone Cable.
    fn owner_pc() -> Vec<DeviceInfo> {
        vec![
            dev(
                Backend::Wasapi,
                "Microphone (Steam Streaming Microphone)",
                "Steam Streaming Microphone",
                Bus::Virtual,
                None,
                r"ROOT\STEAMSTREAMINGMICROPHONE\0000",
            ),
            dev(
                Backend::Wasapi,
                "Line (NUX Audio)",
                "NUX Audio",
                Bus::Usb,
                Some((0x1FC9, 0x8260)),
                r"TUSBAUDIO_ENUM\VID_1FC9&PID_8260&KS\2412191659",
            ),
            dev(
                Backend::Wasapi,
                "Microphone (Anker PowerConf C200)",
                "Anker PowerConf C200",
                Bus::Usb,
                Some((0x291A, 0x3369)),
                r"USB\VID_291A&PID_3369&MI_02\7&22D11071&0&0002",
            ),
            dev(Backend::Asio, "NUX Audio", "", Bus::Unknown, None, ""),
            dev(
                Backend::System,
                "System default input (Microphone (Anker PowerConf C200))",
                "",
                Bus::Unknown,
                None,
                "",
            ),
            dev(Backend::Null, "Test signal", "", Bus::Unknown, None, ""),
        ]
    }

    #[test]
    fn owner_pc_picks_the_nux_asio_driver() {
        let r = rank_inputs(&owner_pc(), None);
        assert_eq!(r[0].device.name, "NUX Audio");
        assert_eq!(r[0].device.id.backend, Backend::Asio);
        assert_eq!(r[1].device.name, "Line (NUX Audio)");
        let anker = r.iter().find(|x| x.device.name.contains("Anker")).unwrap();
        assert_eq!(anker.kind, InputKind::Microphone);
        let steam = r.iter().find(|x| x.device.name.contains("Steam")).unwrap();
        assert_eq!(steam.kind, InputKind::Virtual);
        assert!(r.last().unwrap().kind == InputKind::Test);
    }

    #[test]
    fn real_tone_cable_wins() {
        let mut d = owner_pc();
        d.push(dev(
            Backend::Wasapi,
            "Microphone (Rocksmith Guitar Adapter Mono)",
            "Rocksmith Guitar Adapter",
            Bus::Usb,
            Some(RTC_VID_PID),
            r"USB\VID_12BA&PID_00FF&MI_00\7&1",
        ));
        let r = rank_inputs(&d, None);
        assert_eq!(r[0].kind, InputKind::RealToneCable);
    }

    #[test]
    fn rs_asio_setup_is_preferred_over_other_drivers() {
        let mut d = owner_pc();
        d.push(dev(
            Backend::Asio,
            "Focusrite USB ASIO",
            "",
            Bus::Unknown,
            None,
            "",
        ));
        let setup = RocksmithSetup {
            rs_asio: Some(super::super::rocksmith::RsAsio {
                enable_asio: true,
                inputs: vec![("NUX Audio".into(), 0)],
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = rank_inputs(&d, Some(&setup));
        assert_eq!(r[0].device.name, "NUX Audio");
        assert!(r[0].reason.contains("RS_ASIO"));
    }

    #[test]
    fn channel_suggestion() {
        assert_eq!(suggest_channel(&[0.001, 0.0]), None);
        assert_eq!(suggest_channel(&[0.01, 0.4]), Some(1));
        assert_eq!(suggest_channel(&[0.3, 0.35]), Some(-1));
    }
}
