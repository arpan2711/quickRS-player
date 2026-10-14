//! The owner's Rocksmith 2014 audio setup, read from the game's folder so quickRS starts on the same devices:
//! `Rocksmith.ini` ([Audio]: ExclusiveMode, LatencyBuffer, EnableMicrophone, MonoToStereoChannel, ...) and
//! RS_ASIO's `RS_ASIO.ini` ([Config], [Asio], [Asio.Output], [Asio.Input.0/1/Mic]).
//! Found next to the configured library (the folder holding `dlc`) or in Steam's library folders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RocksmithSetup {
    pub folder: PathBuf,
    /// Rocksmith.ini [Audio]
    pub exclusive_mode: bool,
    /// 1 (lowest) .. 4
    pub latency_buffer: u8,
    pub microphone: bool,
    pub real_tone_cable_only: bool,
    /// MonoToStereoChannel: which channel of a stereo input is the guitar (0 = left).
    pub mono_channel: u8,
    pub ultra_low_latency: bool,
    /// RS_ASIO, when installed (RS_ASIO.dll + RS_ASIO.ini next to the game).
    pub rs_asio: Option<RsAsio>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RsAsio {
    pub enable_asio: bool,
    pub enable_wasapi_outputs: bool,
    pub enable_wasapi_inputs: bool,
    /// "driver", "host" or "custom"
    pub buffer_mode: String,
    pub custom_buffer: Option<u32>,
    /// ASIO driver for the game's output ("" = WASAPI output).
    pub output_driver: String,
    pub output_base_channel: u32,
    /// Guitar inputs: (driver, channel) for Asio.Input.0 and .1.
    pub inputs: Vec<(String, u32)>,
    pub mic: Option<(String, u32)>,
}

/// INI sections → key → value (keys lowercased; `;` and `#` comments).
pub fn parse_ini(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with(';') || l.starts_with('#') {
            continue;
        }
        if l.starts_with('[') && l.ends_with(']') {
            section = l[1..l.len() - 1].trim().to_lowercase();
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let v = v.split(';').next().unwrap_or("").trim().to_string();
            out.entry(section.clone())
                .or_default()
                .insert(k.trim().to_lowercase(), v);
        }
    }
    out
}

fn flag(m: Option<&HashMap<String, String>>, k: &str) -> bool {
    m.and_then(|s| s.get(k))
        .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn num(m: Option<&HashMap<String, String>>, k: &str) -> Option<i64> {
    m.and_then(|s| s.get(k)).and_then(|v| v.trim().parse().ok())
}

fn text(m: Option<&HashMap<String, String>>, k: &str) -> String {
    m.and_then(|s| s.get(k)).cloned().unwrap_or_default()
}

pub fn read_setup(folder: &Path) -> Option<RocksmithSetup> {
    let ini = std::fs::read_to_string(folder.join("Rocksmith.ini")).ok()?;
    let r = parse_ini(&ini);
    let audio = r.get("audio");
    let mut s = RocksmithSetup {
        folder: folder.to_path_buf(),
        exclusive_mode: flag(audio, "exclusivemode"),
        latency_buffer: num(audio, "latencybuffer").unwrap_or(4).clamp(1, 16) as u8,
        microphone: flag(audio, "enablemicrophone"),
        real_tone_cable_only: flag(audio, "realtonecableonly"),
        mono_channel: num(audio, "monotostereochannel").unwrap_or(0).clamp(0, 1) as u8,
        ultra_low_latency: flag(audio, "win32ultralowlatencymode"),
        rs_asio: None,
    };
    if folder.join("RS_ASIO.dll").is_file()
        && let Ok(t) = std::fs::read_to_string(folder.join("RS_ASIO.ini"))
    {
        let a = parse_ini(&t);
        let cfg = a.get("config");
        let asio = a.get("asio");
        let input = |name: &str| -> Option<(String, u32)> {
            let sec = a.get(name);
            let d = text(sec, "driver");
            (!d.is_empty()).then(|| (d, num(sec, "channel").unwrap_or(0).max(0) as u32))
        };
        s.rs_asio = Some(RsAsio {
            enable_asio: flag(cfg, "enableasio"),
            enable_wasapi_outputs: flag(cfg, "enablewasapioutputs"),
            enable_wasapi_inputs: flag(cfg, "enablewasapiinputs"),
            buffer_mode: text(asio, "buffersizemode").to_lowercase(),
            custom_buffer: num(asio, "custombuffersize").filter(|n| *n > 0).map(|n| n as u32),
            output_driver: text(a.get("asio.output"), "driver"),
            output_base_channel: num(a.get("asio.output"), "basechannel").unwrap_or(0).max(0) as u32,
            inputs: ["asio.input.0", "asio.input.1"]
                .iter()
                .filter_map(|n| input(n))
                .collect(),
            mic: input("asio.input.mic"),
        });
    }
    Some(s)
}

/// Steam library folders (from libraryfolders.vdf).
fn steam_libraries() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from(r"C:\Program Files (x86)\Steam"),
        PathBuf::from(r"C:\Program Files\Steam"),
    ];
    if let Ok(pf) = std::env::var("ProgramFiles(x86)") {
        roots.push(PathBuf::from(pf).join("Steam"));
    }
    let mut out = Vec::new();
    for r in roots {
        let vdf = r.join("steamapps").join("libraryfolders.vdf");
        if let Ok(t) = std::fs::read_to_string(&vdf) {
            for line in t.lines() {
                let l = line.trim();
                if let Some(rest) = l.strip_prefix("\"path\"") {
                    let p = rest.trim().trim_matches('"').replace("\\\\", "\\");
                    out.push(PathBuf::from(p));
                }
            }
        }
        if r.is_dir() {
            out.push(r);
        }
    }
    out
}

/// Where Rocksmith 2014 might be installed: the parents of library folders named `dlc`, then Steam.
pub fn candidate_folders(library_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in library_dirs {
        let mut cur = Some(d.as_path());
        for _ in 0..3 {
            let Some(c) = cur else { break };
            if c.join("Rocksmith2014.exe").is_file() || c.join("Rocksmith.ini").is_file() {
                out.push(c.to_path_buf());
                break;
            }
            cur = c.parent();
        }
    }
    for lib in steam_libraries() {
        let p = lib.join("steamapps").join("common").join("Rocksmith2014");
        if p.is_dir() {
            out.push(p);
        }
    }
    out.dedup();
    out
}

/// The first Rocksmith setup found.
pub fn find_setup(library_dirs: &[PathBuf]) -> Option<RocksmithSetup> {
    candidate_folders(library_dirs).iter().find_map(|f| read_setup(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RS_ASIO_INI: &str = "# comment\n[Config]\nEnableWasapiOutputs=1\nEnableWasapiInputs=0\nEnableAsio=1\n\n[Asio]\n; modes\nBufferSizeMode=custom\nCustomBufferSize=64\n\n[Asio.Output]\nDriver=\nBaseChannel=0\n\n[Asio.Input.0]\nDriver=NUX Audio\nChannel=0\n\n[Asio.Input.1]\nDriver=\nChannel=1\n";
    const ROCKSMITH_INI: &str = "[Audio]\nEnableMicrophone=0\nExclusiveMode=1\nLatencyBuffer=2\nRealToneCableOnly=0\nMonoToStereoChannel=0\nWin32UltraLowLatencyMode=1\n[Renderer.Win32]\nScreenWidth=1920\n";

    #[test]
    fn owner_setup_parses() {
        let dir = std::env::temp_dir().join(format!("qrs-rs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Rocksmith.ini"), ROCKSMITH_INI).unwrap();
        std::fs::write(dir.join("RS_ASIO.ini"), RS_ASIO_INI).unwrap();
        std::fs::write(dir.join("RS_ASIO.dll"), b"x").unwrap();
        let s = read_setup(&dir).unwrap();
        assert!(s.exclusive_mode && !s.microphone && s.ultra_low_latency);
        assert_eq!(s.latency_buffer, 2);
        let a = s.rs_asio.unwrap();
        assert!(a.enable_asio && a.enable_wasapi_outputs && !a.enable_wasapi_inputs);
        assert_eq!(a.buffer_mode, "custom");
        assert_eq!(a.custom_buffer, Some(64));
        assert_eq!(a.inputs, vec![("NUX Audio".to_string(), 0)]);
        assert!(a.output_driver.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn finds_the_game_above_the_dlc_folder() {
        let dir = std::env::temp_dir().join(format!("qrs-rs2-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dlc")).unwrap();
        std::fs::write(dir.join("Rocksmith.ini"), ROCKSMITH_INI).unwrap();
        let found = candidate_folders(&[dir.join("dlc")]);
        assert_eq!(found.first(), Some(&dir));
        std::fs::remove_dir_all(dir).ok();
    }
}
