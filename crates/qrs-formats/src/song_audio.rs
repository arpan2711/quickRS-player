//! Find a song's audio (the full song, or the short preview clip Rocksmith plays in its song list) and hand it
//! over as a standard encoded file: Ogg Vorbis for psarc songs (rebuilt losslessly from Wwise's .wem, see `wem`),
//! the stem as stored for feedpaks (Ogg, WAV, MP3, FLAC, Opus). Port of rsplay/audio.py's lookup.
//!
//! psarc: the song's Wwise sound bank (.bnk) names the ids of its .wem files; the largest referenced .wem is the
//! song. RS1-compatibility songs keep their charts in dlc/rs1compatibility*_p.psarc and their audio in the game's
//! songs.psarc, so those archives are searched too.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::package::{Format, Package};
use crate::psarc::Psarc;
use crate::wem;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Ogg,
    Wav,
    Mp3,
    Flac,
    Opus,
    Unknown,
}

impl Codec {
    pub fn from_name(name: &str) -> Codec {
        match name.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
            "ogg" | "oga" => Codec::Ogg,
            "wav" | "wave" => Codec::Wav,
            "mp3" => Codec::Mp3,
            "flac" => Codec::Flac,
            "opus" => Codec::Opus,
            _ => Codec::Unknown,
        }
    }

    /// From the file's first bytes (feedpak stems can have any name).
    pub fn sniff(data: &[u8]) -> Codec {
        if data.starts_with(b"OggS") {
            if data.len() > 36 && &data[28..36] == b"OpusHead" {
                Codec::Opus
            } else {
                Codec::Ogg
            }
        } else if data.starts_with(b"RIFF") && data.len() > 12 && &data[8..12] == b"WAVE" {
            Codec::Wav
        } else if data.starts_with(b"fLaC") {
            Codec::Flac
        } else if data.starts_with(b"ID3") || (data.len() > 2 && data[0] == 0xFF && data[1] & 0xE0 == 0xE0) {
            Codec::Mp3
        } else {
            Codec::Unknown
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Codec::Ogg => "ogg",
            Codec::Wav => "wav",
            Codec::Mp3 => "mp3",
            Codec::Flac => "flac",
            Codec::Opus => "opus",
            Codec::Unknown => "bin",
        }
    }
}

/// An encoded audio file, ready for a decoder.
#[derive(Clone, Debug)]
pub struct EncodedAudio {
    pub data: Vec<u8>,
    pub codec: Codec,
    /// Where it came from, for messages ("audio/windows/123.wem in x_p.psarc").
    pub origin: String,
}

/// Name of the largest .wem in the archive that the Wwise bank references, if the bank is in it.
pub fn bank_wem(archive: &mut Psarc, bank: &str) -> Option<String> {
    let data = archive.read(&format!("audio/windows/{bank}")).ok()?;
    let mut ids = std::collections::HashSet::new();
    for w in data.windows(4) {
        ids.insert(u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    }
    let mut best: Option<(String, u64)> = None;
    for e in archive.find("audio/windows/*.wem") {
        let file = e.name.rsplit('/').next().unwrap_or("");
        let stem = &file[..file.len().saturating_sub(4)];
        if !stem.is_empty()
            && stem.chars().all(|c| c.is_ascii_digit())
            && stem.parse::<u32>().map(|id| ids.contains(&id)).unwrap_or(false)
            && best.as_ref().is_none_or(|b| e.length > b.1)
        {
            best = Some((e.name.clone(), e.length));
        }
    }
    best.map(|b| b.0)
}

/// Archives other than the package that may hold its audio: the game's songs.psarc and RS1 compatibility packs.
pub fn other_audio_archives(pkg_path: &Path) -> Vec<PathBuf> {
    let parent = pkg_path.parent().unwrap_or(Path::new("."));
    let game = if parent
        .file_name()
        .map(|n| n.to_string_lossy().eq_ignore_ascii_case("dlc"))
        .unwrap_or(false)
    {
        parent.parent().unwrap_or(parent).to_path_buf()
    } else {
        parent.to_path_buf()
    };
    let mut out = vec![game.join("songs.psarc")];
    if let Ok(rd) = std::fs::read_dir(game.join("dlc")) {
        let mut rs1: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                let n = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                n.starts_with("rs1compatibility") && n.ends_with(".psarc")
            })
            .collect();
        rs1.sort();
        out.extend(rs1);
    }
    let me = pkg_path.canonicalize().ok();
    out.into_iter()
        .filter(|p| p.is_file() && p.canonicalize().ok() != me)
        .collect()
}

/// The song's audio (or its preview clip) as an encoded file. A feedpak without a preview gives None for the
/// preview (callers cut one from the song).
pub fn song_audio(pkg: &mut Package, preview: bool) -> Result<Option<EncodedAudio>> {
    match pkg.format() {
        Format::Feedpak => {
            let fp = pkg.feedpak().unwrap();
            let rel = if preview { fp.preview() } else { fp.song_stem() };
            let Some(rel) = rel else {
                return if preview {
                    Ok(None)
                } else {
                    Err(Error::Feedpak("this feedpak has no audio stem".into()))
                };
            };
            let data = fp.read(&rel)?;
            let codec = match Codec::sniff(&data) {
                Codec::Unknown => Codec::from_name(&rel),
                c => c,
            };
            Ok(Some(EncodedAudio {
                data,
                codec,
                origin: rel,
            }))
        }
        Format::Psarc => {
            let loc = wem_location(pkg, preview)?;
            if preview && wem_location(pkg, false).ok().as_ref() == Some(&loc) {
                // some custom songs' preview bank names the full song: no real preview (callers cut one)
                return Ok(None);
            }
            let (data, origin) = read_wem(pkg, &loc)?;
            let ogg = wem::wem_to_ogg(&data).map_err(|e| Error::Wem(format!("{}: {e}", loc.name)))?;
            Ok(Some(EncodedAudio {
                data: ogg,
                codec: Codec::Ogg,
                origin,
            }))
        }
    }
}

/// Where a song's .wem is: in the package itself (archive None) or in another of the game's archives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WemLocation {
    pub archive: Option<PathBuf>,
    pub name: String,
}

/// Find the song's audio or preview .wem in a psarc package (and the game's shared archives).
pub fn wem_location(pkg: &mut Package, preview: bool) -> Result<WemLocation> {
    let fallback = pkg.song_keys().len() <= 1;
    wem_location_with(pkg, preview, fallback)
}

/// As `wem_location`, saying whether a package whose banks don't name the audio falls back to its largest .wem
/// (the smallest for the preview). The exporter says no for a multi-song pack, where that's some other song's.
pub fn wem_location_with(pkg: &mut Package, preview: bool, largest_fallback: bool) -> Result<WemLocation> {
    let bank = pkg
        .arrangements
        .iter()
        .map(|a| {
            if preview {
                a.preview_bank.clone()
            } else {
                a.bank.clone()
            }
        })
        .find(|b| !b.is_empty())
        .unwrap_or_default();
    let what = if preview { "preview" } else { "audio" };
    if !bank.is_empty() {
        let archive = pkg.psarc().ok_or_else(|| Error::Psarc("not a psarc".into()))?;
        if let Some(name) = bank_wem(archive, &bank) {
            return Ok(WemLocation { archive: None, name });
        }
        let key = pkg.keys.psarc;
        for other in other_audio_archives(&pkg.path) {
            let Ok(mut a) = Psarc::open(&other, key.as_ref()) else {
                continue;
            };
            if let Some(name) = bank_wem(&mut a, &bank) {
                return Ok(WemLocation {
                    archive: Some(other),
                    name,
                });
            }
        }
    }
    if largest_fallback {
        let files = pkg.audio_files();
        if !files.is_empty() {
            // largest .wem is the full song; the small one is the preview
            let name = if preview && files.len() > 1 {
                files[files.len() - 1].0.clone()
            } else {
                files[0].0.clone()
            };
            return Ok(WemLocation { archive: None, name });
        }
    }
    Err(Error::Psarc(format!("couldn't find this song's {what}")))
}

/// The .wem bytes at a location, and a description of where they came from.
pub fn read_wem(pkg: &mut Package, loc: &WemLocation) -> Result<(Vec<u8>, String)> {
    match &loc.archive {
        None => {
            let pkg_name = pkg
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let data = pkg
                .psarc()
                .ok_or_else(|| Error::Psarc("not a psarc".into()))?
                .read(&loc.name)?;
            Ok((data, format!("{} in {pkg_name}", loc.name)))
        }
        Some(p) => {
            let mut a = Psarc::open(p, pkg.keys.psarc.as_ref())?;
            let o = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok((a.read(&loc.name)?, format!("{} in {o}", loc.name)))
        }
    }
}
