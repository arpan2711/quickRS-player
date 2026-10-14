//! Decode a song's audio to PCM, and bring it to the engine's format (stereo at the engine rate). Replaces
//! rsplay/audio.py's vgmstream step and feedpak's pedalboard decode.
//!
//! Ogg Vorbis (every psarc song after the lossless .wem rewrap, and most feedpak stems) is decoded with lewton;
//! WAV, MP3 and FLAC with symphonia. Opus has no pure-Rust decoder yet and is reported as unsupported.

use std::io::Cursor;
use std::sync::Arc;

use qrs_formats::song_audio::{Codec, EncodedAudio};

#[derive(Debug)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

fn derr(m: impl std::fmt::Display) -> DecodeError {
    DecodeError(m.to_string())
}

/// Decoded audio: interleaved f32 in [-1, 1].
#[derive(Clone, Debug, Default)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels as usize
        }
    }
    pub fn duration(&self) -> f64 {
        if self.rate == 0 {
            0.0
        } else {
            self.frames() as f64 / self.rate as f64
        }
    }
}

pub fn decode(audio: &EncodedAudio) -> Result<Pcm, DecodeError> {
    match audio.codec {
        Codec::Ogg => {
            decode_vorbis(&audio.data).or_else(|e| decode_symphonia(&audio.data, "ogg").map_err(|_| e))
        }
        Codec::Opus => Err(derr(format!("{}: Opus audio isn't supported yet", audio.origin))),
        c => decode_symphonia(&audio.data, c.ext()),
    }
}

/// Ogg Vorbis with lewton (it trims the last packet to the stream's final granule position, as libvorbis does).
pub fn decode_vorbis(data: &[u8]) -> Result<Pcm, DecodeError> {
    use lewton::inside_ogg::OggStreamReader;
    use lewton::samples::InterleavedSamples;
    let mut r = OggStreamReader::new(Cursor::new(data)).map_err(|e| derr(format!("Ogg Vorbis: {e}")))?;
    let rate = r.ident_hdr.audio_sample_rate;
    let channels = r.ident_hdr.audio_channels as u16;
    let mut samples = Vec::new();
    loop {
        match r.read_dec_packet_generic::<InterleavedSamples<f32>>() {
            Ok(Some(p)) => samples.extend_from_slice(&p.samples),
            Ok(None) => break,
            Err(e) => {
                if samples.is_empty() {
                    return Err(derr(format!("Ogg Vorbis: {e}")));
                }
                break; // a damaged tail: keep what decoded
            }
        }
    }
    Ok(Pcm {
        rate,
        channels,
        samples,
    })
}

pub fn decode_symphonia(data: &[u8], ext: &str) -> Result<Pcm, DecodeError> {
    use symphonia::core::codecs::CodecParameters;
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let mss = MediaSourceStream::new(Box::new(Cursor::new(data.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(ext);
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| derr(format!("{ext}: {e}")))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| derr(format!("{ext}: no audio track")))?
        .clone();
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err(derr(format!("{ext}: no audio codec parameters")));
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| derr(format!("{ext}: {e}")))?;
    let mut rate = params.sample_rate.unwrap_or(0);
    let mut channels = params.channels.as_ref().map(|c| c.count() as u16).unwrap_or(0);
    let mut samples: Vec<f32> = Vec::new();
    let mut chunk: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => {
                if samples.is_empty() {
                    return Err(derr(format!("{ext}: {e}")));
                }
                break;
            }
        };
        if packet.track_id != track.id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                rate = buf.spec().rate();
                channels = buf.spec().channels().count() as u16;
                chunk.clear();
                buf.copy_to_vec_interleaved(&mut chunk);
                samples.extend_from_slice(&chunk);
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue, // skip a damaged packet
            Err(e) => return Err(derr(format!("{ext}: {e}"))),
        }
    }
    if rate == 0 || channels == 0 {
        return Err(derr(format!("{ext}: unknown sample rate or channel count")));
    }
    Ok(Pcm {
        rate,
        channels,
        samples,
    })
}

/// The song as the engine plays it: interleaved stereo i16 at the engine rate. Shared between threads.
#[derive(Clone, Debug)]
pub struct SongBuffer {
    pub rate: u32,
    pub samples: Arc<Vec<i16>>,
}

impl SongBuffer {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.rate as f64
    }
    pub fn silent(rate: u32, seconds: f64) -> SongBuffer {
        SongBuffer {
            rate,
            samples: Arc::new(vec![0; 2 * (rate as f64 * seconds) as usize]),
        }
    }
}

/// Stereo f32: mono is duplicated, more channels are folded down (odd ones left, even ones right).
pub fn to_stereo(pcm: &Pcm) -> Vec<f32> {
    let ch = pcm.channels as usize;
    match ch {
        2 => pcm.samples.clone(),
        1 => pcm.samples.iter().flat_map(|&s| [s, s]).collect(),
        0 => Vec::new(),
        _ => {
            let mut out = Vec::with_capacity(pcm.frames() * 2);
            let scale = 2.0 / ch as f32;
            for f in pcm.samples.chunks_exact(ch) {
                let (mut l, mut r) = (0.0, 0.0);
                for (k, s) in f.iter().enumerate() {
                    if k % 2 == 0 { l += s } else { r += s }
                }
                out.push(l * scale);
                out.push(r * scale);
            }
            out
        }
    }
}

/// Resample interleaved audio between fixed rates (offline, high quality FFT resampler).
pub fn resample(samples: Vec<f32>, channels: usize, from: u32, to: u32) -> Result<Vec<f32>, DecodeError> {
    use rubato::audioadapter_buffers::owned::InterleavedOwned;
    use rubato::{Fft, FixedSync, Resampler};
    if from == to || samples.is_empty() {
        return Ok(samples);
    }
    let frames = samples.len() / channels;
    let input = InterleavedOwned::new_from(samples, channels, frames).map_err(derr)?;
    let mut r = Fft::<f32>::new(from as usize, to as usize, 1024, channels, FixedSync::Both).map_err(derr)?;
    let out = r.process_all(&input, frames, None).map_err(derr)?;
    Ok(out.take_data())
}

/// f32 to i16 the way vgmstream does it: scale by 32767 and truncate toward zero (`as` saturates). Measured on
/// real songs against vgmstream's own WAVs: 99.8% of samples identical, all within 1 LSB.
pub fn f32_to_i16(s: f32) -> i16 {
    (s * 32767.0) as i16
}

/// Decoded audio in the engine's format.
pub fn to_song_buffer(pcm: Pcm, rate: u32) -> Result<SongBuffer, DecodeError> {
    let from = pcm.rate;
    let stereo = to_stereo(&pcm);
    drop(pcm);
    let stereo = resample(stereo, 2, from, rate)?;
    let samples: Vec<i16> = stereo.iter().map(|&s| f32_to_i16(s)).collect();
    Ok(SongBuffer {
        rate,
        samples: Arc::new(samples),
    })
}

// --- WAV (16-bit PCM) ---------------------------------------------------------------------------------------
pub fn write_wav_i16(
    path: &std::path::Path,
    rate: u32,
    channels: u16,
    samples: &[i16],
) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out)
}

/// A 16-bit PCM WAV: (rate, channels, samples).
pub fn read_wav_i16(data: &[u8]) -> Result<(u32, u16, Vec<i16>), DecodeError> {
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(derr("not a WAV file"));
    }
    let mut pos = 12;
    let (mut rate, mut channels, mut bits) = (0u32, 0u16, 0u16);
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &data[pos + 8..(pos + 8 + size).min(data.len())];
        if id == b"fmt " && body.len() >= 16 {
            channels = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            if bits != 16 {
                return Err(derr(format!("{bits}-bit WAV (only 16-bit here)")));
            }
            let s = body
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            return Ok((rate, channels, s));
        }
        pos += 8 + size + (size & 1);
    }
    Err(derr("WAV without data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_folding() {
        let mono = Pcm {
            rate: 8000,
            channels: 1,
            samples: vec![0.5, -0.5],
        };
        assert_eq!(to_stereo(&mono), vec![0.5, 0.5, -0.5, -0.5]);
        let quad = Pcm {
            rate: 8000,
            channels: 4,
            samples: vec![0.1, 0.2, 0.3, 0.4],
        };
        let s = to_stereo(&quad);
        assert!((s[0] - 0.2).abs() < 1e-6 && (s[1] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn resample_length_and_tone() {
        // one second of a 1 kHz tone at 44.1 kHz -> 48 kHz
        let n = 44100;
        let mono: Vec<f32> = (0..n)
            .map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / 44100.0).sin() * 0.5)
            .collect();
        let out = resample(mono, 1, 44100, 48000).unwrap();
        assert!((out.len() as i64 - 48000).abs() <= 2, "{}", out.len());
        let peak = out[1000..47000].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.01, "{peak}");
    }

    #[test]
    fn wav_roundtrip() {
        let dir = std::env::temp_dir().join(format!("qrs-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        write_wav_i16(&p, 48000, 2, &[1, -2, 3, -4]).unwrap();
        let (r, c, s) = read_wav_i16(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!((r, c, s), (48000, 2, vec![1, -2, 3, -4]));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn conversion_clips() {
        assert_eq!(f32_to_i16(1.5), 32767);
        assert_eq!(f32_to_i16(-1.5), -32768);
        assert_eq!(f32_to_i16(0.5), 16383);
        assert_eq!(f32_to_i16(-0.5), -16383);
    }
}
