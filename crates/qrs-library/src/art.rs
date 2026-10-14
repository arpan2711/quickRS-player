//! Album covers. Port of rsplay/preview.py's ArtCache extraction, for psarc and feedpak songs.
//!
//! psarc: gfxassets/album_art/album_<key>_{64,128,256}.dds textures (DXT1 in Rocksmith's own songs; DXT3/DXT5 or
//! uncompressed in some custom songs), decoded as Pillow decodes them. feedpak: the manifest's `cover` (PNG, JPEG
//! or WebP).
//! Covers are decoded once and cached as PNGs, named like the Python app's cache (`<song id>.png` and
//! `<song id>@256.png`, with unsafe characters folded to `_`), so that cache can be reused as is.

use std::path::{Path, PathBuf};

use image::{RgbaImage, imageops::FilterType};
use qrs_formats::{Package, package::Format};

/// A DDS texture as RGBA (the top mip level). DXT1/DXT3/DXT5 and uncompressed 24/32-bit RGB(A).
pub fn decode_dds(data: &[u8]) -> Option<RgbaImage> {
    if data.len() < 128 || &data[..4] != b"DDS " {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    let height = u32_at(12) as usize;
    let width = u32_at(16) as usize;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return None;
    }
    let pf_flags = u32_at(80);
    let fourcc = &data[84..88];
    let mut body = &data[128..];
    let mut rgba = vec![0u8; width * height * 4];
    if pf_flags & 0x4 != 0 {
        let mut fmt = fourcc.to_vec();
        if fourcc == b"DX10" {
            if data.len() < 148 {
                return None;
            }
            body = &data[148..];
            // DXGI formats: 71/72 BC1, 74/75 BC2, 77/78 BC3
            fmt = match u32_at(128) {
                71 | 72 => b"DXT1".to_vec(),
                74 | 75 => b"DXT3".to_vec(),
                77 | 78 => b"DXT5".to_vec(),
                _ => return None,
            };
        }
        let (block_bytes, dec): (usize, fn(&[u8], &mut Block)) = match fmt.as_slice() {
            b"DXT1" => (8, bc1_block),
            b"DXT2" | b"DXT3" => (16, bc2_block),
            b"DXT4" | b"DXT5" => (16, bc3_block),
            _ => return None,
        };
        let (bw, bh) = (width.div_ceil(4), height.div_ceil(4));
        if body.len() < bw * bh * block_bytes {
            return None;
        }
        let mut block: Block = [[0; 4]; 16];
        for by in 0..bh {
            for bx in 0..bw {
                let k = (by * bw + bx) * block_bytes;
                dec(&body[k..k + block_bytes], &mut block);
                for y in 0..4 {
                    let py = by * 4 + y;
                    if py >= height {
                        break;
                    }
                    for x in 0..4 {
                        let px = bx * 4 + x;
                        if px >= width {
                            break;
                        }
                        let dst = (py * width + px) * 4;
                        rgba[dst..dst + 4].copy_from_slice(&block[y * 4 + x]);
                    }
                }
            }
        }
    } else if pf_flags & 0x40 != 0 {
        // uncompressed RGB(A) with bit masks
        let bits = u32_at(88) as usize;
        let (rm, gm, bm, am) = (
            u32_at(92),
            u32_at(96),
            u32_at(100),
            if pf_flags & 0x1 != 0 { u32_at(104) } else { 0 },
        );
        let bpp = bits / 8;
        if !(bpp == 3 || bpp == 4) || body.len() < width * height * bpp {
            return None;
        }
        let chan = |v: u32, m: u32| -> u8 {
            if m == 0 {
                return 255;
            }
            let shift = m.trailing_zeros();
            let max = m >> shift;
            (((v & m) >> shift) * 255 / max.max(1)) as u8
        };
        for i in 0..width * height {
            let p = &body[i * bpp..i * bpp + bpp];
            let v = if bpp == 3 {
                u32::from_le_bytes([p[0], p[1], p[2], 0])
            } else {
                u32::from_le_bytes([p[0], p[1], p[2], p[3]])
            };
            rgba[i * 4] = chan(v, rm);
            rgba[i * 4 + 1] = chan(v, gm);
            rgba[i * 4 + 2] = chan(v, bm);
            rgba[i * 4 + 3] = if am == 0 { 255 } else { chan(v, am) };
        }
    } else {
        return None;
    }
    RgbaImage::from_raw(width as u32, height as u32, rgba)
}

// -- BCn blocks, as Pillow's BcnDecode.c decodes them --------------------------------------------------------
// The Python apps read covers with Pillow, which mixes the 8-bit endpoint colours with truncating division
// (other decoders mix in 5/6-bit space and round: up to 1 apart). Decoding the same way gives the same pixels.

type Block = [[u8; 4]; 16];

fn rgb565(c: u16) -> [u8; 4] {
    let r = ((c & 0xf800) >> 8) as u8;
    let g = ((c & 0x07e0) >> 3) as u8;
    let b = ((c & 0x001f) << 3) as u8;
    [r | (r >> 5), g | (g >> 6), b | (b >> 5), 0xff]
}

/// The colour half of a block. BC2 and BC3 always use four colours (`separate_alpha`).
fn bc1_colors(src: &[u8], out: &mut Block, separate_alpha: bool) {
    let c0 = u16::from_le_bytes([src[0], src[1]]);
    let c1 = u16::from_le_bytes([src[2], src[3]]);
    let lut = u32::from_le_bytes([src[4], src[5], src[6], src[7]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |k: usize, w0: u16, w1: u16, d: u16| ((w0 * p0[k] as u16 + w1 * p1[k] as u16) / d) as u8;
    let mut p = [p0, p1, [0, 0, 0, 0xff], [0, 0, 0, 0xff]];
    if c0 > c1 || separate_alpha {
        for k in 0..3 {
            p[2][k] = mix(k, 2, 1, 3);
            p[3][k] = mix(k, 1, 2, 3);
        }
    } else {
        for k in 0..3 {
            p[2][k] = mix(k, 1, 1, 2);
        }
        p[3] = [0, 0, 0, 0]; // transparent black
    }
    for (n, px) in out.iter_mut().enumerate() {
        *px = p[((lut >> (2 * n)) & 3) as usize];
    }
}

fn bc1_block(src: &[u8], out: &mut Block) {
    bc1_colors(src, out, false);
}

/// DXT3: explicit 4-bit alpha.
fn bc2_block(src: &[u8], out: &mut Block) {
    bc1_colors(&src[8..], out, true);
    for (n, px) in out.iter_mut().enumerate() {
        let a = (src[n / 2] >> ((n % 2) * 4)) & 0xf;
        px[3] = (a << 4) | a;
    }
}

/// DXT5: interpolated alpha.
fn bc3_block(src: &[u8], out: &mut Block) {
    bc1_colors(&src[8..], out, true);
    let (a0, a1) = (src[0] as u16, src[1] as u16);
    let lut = |o: usize| src[o] as u32 | (src[o + 1] as u32) << 8 | (src[o + 2] as u32) << 16;
    let (lut1, lut2) = (lut(2), lut(5));
    let mut a = [a0 as u8, a1 as u8, 0, 0, 0, 0, 0, 0];
    if a0 > a1 {
        for i in 0..6u16 {
            a[2 + i as usize] = (((6 - i) * a0 + (1 + i) * a1) / 7) as u8;
        }
    } else {
        for i in 0..4u16 {
            a[2 + i as usize] = (((4 - i) * a0 + (1 + i) * a1) / 5) as u8;
        }
        a[7] = 0xff;
    }
    for n in 0..8 {
        out[n][3] = a[((lut1 >> (3 * n)) & 7) as usize];
        out[8 + n][3] = a[((lut2 >> (3 * n)) & 7) as usize];
    }
}

/// The song's cover at `res` x `res` pixels, or None. For 256 px only a real 256 px texture is used (callers then
/// scale the 128 px one), as the Python app did.
pub fn cover(pkg: &mut Package, res: u32) -> Option<RgbaImage> {
    let img = match pkg.format() {
        Format::Feedpak => {
            let bytes = pkg.feedpak()?.cover_bytes()?;
            image::load_from_memory(&bytes).ok()?.to_rgba8()
        }
        Format::Psarc => {
            let key = pkg.song_key.to_lowercase();
            let archive = pkg.psarc()?;
            let mut names: Vec<qrs_formats::psarc::Entry> = if key.is_empty() {
                Vec::new()
            } else {
                archive.find(&format!("gfxassets/album_art/album_{key}_*.dds"))
            };
            if names.is_empty() {
                names = archive.find("gfxassets/album_art/*.dds");
            }
            if res == 256 {
                names.retain(|e| e.name.ends_with("_256.dds"));
            }
            // prefer 128 px; 256 if that's all there is
            names.sort_by_key(|e| {
                if e.name.ends_with("_128.dds") {
                    0
                } else if e.name.ends_with("_256.dds") {
                    1
                } else {
                    2
                }
            });
            let e = names.first()?.clone();
            decode_dds(&archive.read_entry(&e).ok()?)?
        }
    };
    Some(if img.width() == res && img.height() == res {
        img
    } else {
        image::imageops::resize(&img, res, res, FilterType::Lanczos3)
    })
}

/// `re.sub(r"[^A-Za-z0-9._-]+", "_", s)`
pub fn safe_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
            out.push(c);
            in_run = false;
        } else if !in_run {
            out.push('_');
            in_run = true;
        }
    }
    out
}

/// Covers cached as PNG files.
#[derive(Clone, Debug)]
pub struct ArtCache {
    pub dir: PathBuf,
}

impl ArtCache {
    pub fn new(dir: impl Into<PathBuf>) -> ArtCache {
        ArtCache { dir: dir.into() }
    }

    /// The cache file for a song id ("file name|song key") at 128 px or 256 px.
    pub fn file(&self, song_id: &str, res: u32) -> PathBuf {
        let key = if res > 128 {
            format!("{song_id}@256")
        } else {
            song_id.to_string()
        };
        self.dir.join(format!("{}.png", safe_name(&key)))
    }

    /// The cached cover, decoding and caching it first if needed. None when the song has no cover.
    pub fn ensure(
        &self,
        song_id: &str,
        path: &Path,
        song_key: &str,
        keys: &qrs_formats::Keys,
        res: u32,
    ) -> Option<PathBuf> {
        let file = self.file(song_id, res);
        if file.is_file() {
            return Some(file);
        }
        let mut pkg = Package::open(path, keys, (!song_key.is_empty()).then_some(song_key)).ok()?;
        let img = cover(&mut pkg, res)?;
        std::fs::create_dir_all(&self.dir).ok()?;
        let tmp = file.with_extension("tmp.png");
        img.save(&tmp).ok()?;
        std::fs::rename(&tmp, &file).ok()?;
        Some(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxt1_mixes_like_pillow() {
        // a 4x4 DXT1 texture: c0 pure red, c1 = red 1/31; pixels 0-3 use colours 0-3
        let mut dds = vec![0u8; 128];
        dds[..4].copy_from_slice(b"DDS ");
        dds[4..8].copy_from_slice(&124u32.to_le_bytes());
        dds[12..16].copy_from_slice(&4u32.to_le_bytes());
        dds[16..20].copy_from_slice(&4u32.to_le_bytes());
        dds[76..80].copy_from_slice(&32u32.to_le_bytes());
        dds[80..84].copy_from_slice(&4u32.to_le_bytes());
        dds[84..88].copy_from_slice(b"DXT1");
        dds.extend([0x00, 0xf8, 0x00, 0x08, 0xe4, 0, 0, 0]);
        let img = decode_dds(&dds).unwrap();
        let red = |x: u32| img.get_pixel(x, 0).0;
        assert_eq!(red(0), [255, 0, 0, 255]);
        assert_eq!(red(1), [8, 0, 0, 255]);
        assert_eq!(red(2), [172, 0, 0, 255]); // (2 * 255 + 8) / 3 = 172.7, truncated
        assert_eq!(red(3), [90, 0, 0, 255]);
        // c0 <= c1: three colours and transparent black
        dds.truncate(128);
        dds.extend([0x00, 0x08, 0x00, 0xf8, 0xe4, 0, 0, 0]);
        let img = decode_dds(&dds).unwrap();
        assert_eq!(img.get_pixel(2, 0).0, [131, 0, 0, 255]);
        assert_eq!(img.get_pixel(3, 0).0, [0, 0, 0, 0]);
    }

    #[test]
    fn safe_names_match_python() {
        assert_eq!(safe_name("1979_p.psarc|"), "1979_p.psarc_");
        assert_eq!(safe_name("agaithra_p.psarc|@256"), "agaithra_p.psarc_256");
        assert_eq!(
            safe_name("3 Doors Down - Away.feedpak|"),
            "3_Doors_Down_-_Away.feedpak_"
        );
    }

    #[test]
    fn dxt1_block_decodes() {
        // a 4x4 DDS, DXT1, one block: color0 = pure red (0xF800), color1 = black, all indices 0
        let mut d = vec![0u8; 128];
        d[..4].copy_from_slice(b"DDS ");
        d[4..8].copy_from_slice(&124u32.to_le_bytes());
        d[12..16].copy_from_slice(&4u32.to_le_bytes());
        d[16..20].copy_from_slice(&4u32.to_le_bytes());
        d[80..84].copy_from_slice(&4u32.to_le_bytes());
        d[84..88].copy_from_slice(b"DXT1");
        d.extend_from_slice(&[0x00, 0xF8, 0x00, 0x00, 0, 0, 0, 0]);
        let img = decode_dds(&d).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(3, 3).0, [255, 0, 0, 255]);
    }
}
