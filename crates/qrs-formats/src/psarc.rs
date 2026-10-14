//! PSARC archive reader and writer (Rocksmith 2014 flavour). Port of rsplay/psarc.py, the PC/Mac neutral view of
//! rsfeedpak/platform.py (`NeutralArchive`) and its `write_psarc`.
//!
//! Layout (all big-endian):
//!   header (32 bytes)  "PSAR", u16 major, u16 minor, "zlib", u32 toc_length (includes header),
//!                      u32 toc_entry_size (30), u32 toc_entries, u32 block_size, u32 flags
//!   TOC                toc_entries x 30-byte entries, then the block-size table.
//!                      If flags & 4 the whole TOC region is AES-256-CFB (128-bit segments, IV = 0).
//!   data               each entry is a run of blocks starting at its zindex in the block table.
//!
//! Entry 0 is the manifest: newline-separated names of entries 1..n.
//!
//! Mac packages (`*_m.psarc`) keep charts in `songs/bin/macos/` and audio in `audio/mac/`; entries are presented
//! with their PC spelling (`songs/bin/generic/`, `audio/windows/`) so everything above can't tell them apart.
//! The original name is kept in `Entry::raw_name`.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use crate::crypto;
use crate::error::{Error, Result};
use crate::glob;
use crate::keys::Key;

pub const HEADER_SIZE: usize = 32;
pub const ENTRY_SIZE: usize = 30;
pub const FLAG_ENCRYPTED_TOC: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Pc,
    Mac,
}

/// (Mac spelling, PC spelling) of the path prefixes that differ.
const PLATFORM_PATHS: [(&str, &str); 2] = [
    ("songs/bin/macos/", "songs/bin/generic/"),
    ("audio/mac/", "audio/windows/"),
];

pub fn to_pc_name(name: &str) -> String {
    let low = name.to_lowercase();
    for (mac, pc) in PLATFORM_PATHS {
        if low.starts_with(mac) {
            return format!("{pc}{}", &name[mac.len()..]);
        }
    }
    name.to_string()
}

pub fn to_mac_name(name: &str) -> String {
    let low = name.to_lowercase();
    for (mac, pc) in PLATFORM_PATHS {
        if low.starts_with(pc) {
            return format!("{mac}{}", &name[pc.len()..]);
        }
    }
    name.to_string()
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// Position in the TOC (0 is the manifest).
    pub index: usize,
    /// The name as the PC version spells it.
    pub name: String,
    /// The name stored in the archive.
    pub raw_name: String,
    pub md5: [u8; 16],
    pub zindex: u32,
    pub length: u64,
    pub offset: u64,
}

trait Source: Read + Seek + Send {}
impl<T: Read + Seek + Send> Source for T {}

pub struct Psarc {
    src: Box<dyn Source>,
    pub path: Option<PathBuf>,
    pub version: (u16, u16),
    pub block_size: u32,
    pub flags: u32,
    pub encrypted_toc: bool,
    pub platform: Platform,
    pub entries: Vec<Entry>,
    block_table: Vec<u32>,
    by_name: HashMap<String, usize>,
}

impl std::fmt::Debug for Psarc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Psarc")
            .field("path", &self.path)
            .field("version", &self.version)
            .field("platform", &self.platform)
            .field("entries", &self.entries.len())
            .finish()
    }
}

pub fn block_table_width(block_size: u32) -> Result<usize> {
    let mut width = 1usize;
    while 256u64.pow(width as u32) < block_size as u64 {
        width += 1;
    }
    if !(2..=4).contains(&width) {
        return Err(Error::Psarc(format!("unsupported block size {block_size}")));
    }
    Ok(width)
}

fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0u64, |acc, b| (acc << 8) | *b as u64)
}

fn looks_like_zlib(block: &[u8]) -> bool {
    block.len() >= 2 && block[0] == 0x78 && matches!(block[1], 0x01 | 0x5E | 0x9C | 0xDA)
}

fn key_missing() -> Error {
    Error::KeyMissing(
        "This archive has an encrypted table of contents and no PSARC key is configured. \
         Set it in keys.json (see keys.example.json) or via RSPLAY_PSARC_KEY."
            .into(),
    )
}

impl Psarc {
    pub fn open(path: impl AsRef<Path>, key: Option<&Key>) -> Result<Self> {
        let path = path.as_ref();
        let f = File::open(path)?;
        let mut p = Self::from_source(Box::new(BufReader::with_capacity(1 << 16, f)), key)?;
        p.path = Some(path.to_path_buf());
        Ok(p)
    }

    pub fn from_bytes(data: Vec<u8>, key: Option<&Key>) -> Result<Self> {
        Self::from_source(Box::new(Cursor::new(data)), key)
    }

    fn from_source(mut src: Box<dyn Source>, key: Option<&Key>) -> Result<Self> {
        src.seek(SeekFrom::Start(0))?;
        let mut h = [0u8; HEADER_SIZE];
        if src.read_exact(&mut h).is_err() {
            return Err(Error::Psarc("file too short to be a PSARC archive".into()));
        }
        if &h[0..4] != b"PSAR" {
            return Err(Error::Psarc("not a PSARC archive (bad magic)".into()));
        }
        let major = u16::from_be_bytes([h[4], h[5]]);
        let minor = u16::from_be_bytes([h[6], h[7]]);
        if &h[8..12] != b"zlib" {
            return Err(Error::Psarc(format!(
                "unsupported compression method {:?}",
                String::from_utf8_lossy(&h[8..12])
            )));
        }
        let u32_at = |o: usize| u32::from_be_bytes([h[o], h[o + 1], h[o + 2], h[o + 3]]);
        let (toc_length, entry_size, entry_count, block_size, flags) = (
            u32_at(12) as usize,
            u32_at(16) as usize,
            u32_at(20) as usize,
            u32_at(24),
            u32_at(28),
        );
        if entry_size != ENTRY_SIZE {
            return Err(Error::Psarc(format!("unsupported TOC entry size {entry_size}")));
        }
        if toc_length < HEADER_SIZE + entry_count * entry_size {
            return Err(Error::Psarc("TOC length is smaller than its entries".into()));
        }
        let encrypted_toc = flags & FLAG_ENCRYPTED_TOC != 0;
        let mut toc = vec![0u8; toc_length - HEADER_SIZE];
        if src.read_exact(&mut toc).is_err() {
            return Err(Error::Psarc("truncated TOC".into()));
        }
        if encrypted_toc {
            let key = key.ok_or_else(key_missing)?;
            toc = crypto::cfb_decrypt(&toc, key);
        }

        let mut raw = Vec::with_capacity(entry_count);
        for i in 0..entry_count {
            let r = &toc[i * ENTRY_SIZE..(i + 1) * ENTRY_SIZE];
            let mut md5 = [0u8; 16];
            md5.copy_from_slice(&r[..16]);
            raw.push((md5, be(&r[16..20]) as u32, be(&r[20..25]), be(&r[25..30])));
        }
        let width = block_table_width(block_size)?;
        let table_bytes = &toc[entry_count * ENTRY_SIZE..];
        let block_table: Vec<u32> = table_bytes.chunks_exact(width).map(|c| be(c) as u32).collect();
        let file_size = src.seek(SeekFrom::End(0))?;
        for (i, (_, zindex, length, offset)) in raw.iter().enumerate() {
            if *offset > file_size || (*length > 0 && *zindex as usize >= block_table.len()) {
                let hint = if encrypted_toc { " (wrong PSARC key?)" } else { "" };
                return Err(Error::Psarc(format!(
                    "TOC entry {i} points outside the archive{hint}"
                )));
            }
        }

        let mut p = Psarc {
            src,
            path: None,
            version: (major, minor),
            block_size,
            flags,
            encrypted_toc,
            platform: Platform::Pc,
            entries: Vec::new(),
            block_table,
            by_name: HashMap::new(),
        };
        if raw.is_empty() {
            return Ok(p);
        }
        let (md5, zindex, length, offset) = raw[0];
        let manifest = Entry {
            index: 0,
            name: String::new(),
            raw_name: String::new(),
            md5,
            zindex,
            length,
            offset,
        };
        let names_blob = p.read_entry(&manifest)?;
        let names_text = String::from_utf8_lossy(&names_blob);
        let names: Vec<&str> = names_text.lines().collect();
        let is_mac = names.iter().any(|n| {
            let l = n.to_lowercase();
            l.starts_with("songs/bin/macos/") || l.starts_with("audio/mac/")
        });
        p.platform = if is_mac { Platform::Mac } else { Platform::Pc };
        for (i, (md5, zindex, length, offset)) in raw.into_iter().enumerate().skip(1) {
            let raw_name = names
                .get(i - 1)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("_unnamed/entry_{i:05}"));
            let name = to_pc_name(&raw_name);
            p.entries.push(Entry {
                index: i,
                name,
                raw_name,
                md5,
                zindex,
                length,
                offset,
            });
        }
        for (k, e) in p.entries.iter().enumerate() {
            p.by_name.entry(e.name.to_lowercase()).or_insert(k);
        }
        for (k, e) in p.entries.iter().enumerate() {
            p.by_name.entry(e.raw_name.to_lowercase()).or_insert(k);
        }
        Ok(p)
    }

    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }

    /// The entry with this name (either platform's spelling, any case).
    pub fn get(&self, name: &str) -> Result<&Entry> {
        self.by_name
            .get(&name.to_lowercase())
            .map(|&k| &self.entries[k])
            .ok_or_else(|| Error::Psarc(format!("no file named {name:?} in archive")))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(&name.to_lowercase())
    }

    /// Entries whose (PC-spelled) name matches a glob, case-insensitively; `*` also matches `/`.
    pub fn find(&self, pattern: &str) -> Vec<Entry> {
        self.entries
            .iter()
            .filter(|e| glob::matches(pattern, &e.name))
            .cloned()
            .collect()
    }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let e = self.get(name)?.clone();
        self.read_entry(&e)
    }

    pub fn read_entry(&mut self, entry: &Entry) -> Result<Vec<u8>> {
        let label = if entry.name.is_empty() {
            "manifest"
        } else {
            entry.name.as_str()
        };
        let mut out = Vec::with_capacity(entry.length as usize);
        self.src.seek(SeekFrom::Start(entry.offset))?;
        let mut z = entry.zindex as usize;
        let mut chunk = Vec::new();
        while (out.len() as u64) < entry.length {
            let Some(&zsize) = self.block_table.get(z) else {
                return Err(Error::Psarc(format!("{label}: block table overrun")));
            };
            if zsize == 0 {
                // Zero means a full, uncompressed block (the last one may be shorter at the end of the file).
                chunk.resize(self.block_size as usize, 0);
                let n = read_up_to(&mut self.src, &mut chunk)?;
                if n == 0 {
                    return Err(Error::Psarc(format!("{label}: unexpected end of file")));
                }
                out.extend_from_slice(&chunk[..n]);
            } else {
                chunk.resize(zsize as usize, 0);
                if self.src.read_exact(&mut chunk).is_err() {
                    return Err(Error::Psarc(format!("{label}: unexpected end of file")));
                }
                if looks_like_zlib(&chunk) {
                    match inflate(&chunk) {
                        Ok(d) => out.extend_from_slice(&d),
                        // Some raw blocks (e.g. in .wem audio) happen to start like a zlib header.
                        Err(_) => out.extend_from_slice(&chunk),
                    }
                } else {
                    out.extend_from_slice(&chunk);
                }
            }
            z += 1;
        }
        out.truncate(entry.length as usize);
        Ok(out)
    }

    /// Extract entries (optionally filtered by a glob) under `out_dir`, with their stored names. Returns the
    /// written paths.
    pub fn extract(&mut self, out_dir: &Path, pattern: Option<&str>) -> Result<Vec<PathBuf>> {
        let entries: Vec<Entry> = match pattern {
            Some(p) => self.find(p),
            None => self.entries.clone(),
        };
        let mut written = Vec::new();
        for e in entries {
            let target = out_dir.join(safe_relative_path(&e.raw_name)?);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let data = self.read_entry(&e)?;
            std::fs::write(&target, data)?;
            written.push(target);
        }
        Ok(written)
    }
}

fn read_up_to(r: &mut dyn Source, buf: &mut [u8]) -> Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let k = r.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

/// zlib-decompress a complete stream (trailing bytes after the stream end are ignored).
pub fn inflate(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut d = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    d.read_to_end(&mut out)?;
    Ok(out)
}

pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("in-memory write");
    e.finish().expect("in-memory write")
}

/// An archive member name as a relative path with no absolute, `..` or drive parts.
pub fn safe_relative_path(name: &str) -> Result<PathBuf> {
    let norm = name.replace('\\', "/");
    let parts: Vec<String> = norm
        .split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .map(|p| p.replace(':', "_"))
        .collect();
    if parts.is_empty() {
        return Err(Error::Psarc(format!("invalid member name {name:?}")));
    }
    let path: PathBuf = parts.iter().collect();
    debug_assert!(path.components().all(|c| matches!(c, Component::Normal(_))));
    Ok(path)
}

/// A PSARC v1.4 archive holding `files` in the order given (zlib blocks where they shrink, raw otherwise; the TOC
/// encrypted when a key is given). Port of builders.build_psarc / platform.write_psarc.
pub fn write_psarc(
    files: &[(String, Vec<u8>)],
    key: Option<&Key>,
    block_size: u32,
    compress: bool,
) -> Vec<u8> {
    use md5::{Digest, Md5};
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    let manifest = names.join("\n").into_bytes();
    let mut contents: Vec<&[u8]> = vec![&manifest];
    contents.extend(files.iter().map(|(_, d)| d.as_slice()));
    let width = match block_size {
        65536 => 2,
        16777216 => 3,
        _ => 4,
    };
    let (mut table, mut chunks, mut meta) = (Vec::<u32>::new(), Vec::<Vec<u8>>::new(), Vec::new());
    for content in &contents {
        let (zindex, mut stored_total) = (table.len(), 0u64);
        for block in content.chunks(block_size as usize) {
            let z = deflate(block);
            let (stored, size) = if compress && z.len() < block.len() {
                let n = z.len() as u32;
                (z, n)
            } else {
                let n = if block.len() == block_size as usize {
                    0
                } else {
                    block.len() as u32
                };
                (block.to_vec(), n)
            };
            table.push(size);
            stored_total += stored.len() as u64;
            chunks.push(stored);
        }
        meta.push((zindex as u32, content.len() as u64, stored_total));
    }
    let toc_length = HEADER_SIZE + ENTRY_SIZE * contents.len() + width * table.len();
    let mut toc = Vec::with_capacity(toc_length - HEADER_SIZE);
    let mut offset = toc_length as u64;
    for (i, (zindex, length, stored_total)) in meta.iter().enumerate() {
        let md5: [u8; 16] = if i == 0 {
            [0; 16]
        } else {
            Md5::digest(names[i - 1].as_bytes()).into()
        };
        toc.extend_from_slice(&md5);
        toc.extend_from_slice(&zindex.to_be_bytes());
        toc.extend_from_slice(&length.to_be_bytes()[3..]);
        toc.extend_from_slice(&offset.to_be_bytes()[3..]);
        offset += stored_total;
    }
    for v in table {
        toc.extend_from_slice(&v.to_be_bytes()[4 - width..]);
    }
    if let Some(k) = key {
        toc = crypto::cfb_encrypt(&toc, k);
    }
    let mut out = Vec::with_capacity(offset as usize);
    out.extend_from_slice(b"PSAR");
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(b"zlib");
    for v in [
        toc_length as u32,
        ENTRY_SIZE as u32,
        contents.len() as u32,
        block_size,
    ] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&(if key.is_some() { FLAG_ENCRYPTED_TOC } else { 0 }).to_be_bytes());
    out.extend_from_slice(&toc);
    for c in chunks {
        out.extend_from_slice(&c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: Key = {
        let mut k = [0u8; 32];
        let mut i = 0;
        while i < 32 {
            k[i] = i as u8;
            i += 1;
        }
        k
    };

    fn sample() -> Vec<(String, Vec<u8>)> {
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let noise: Vec<u8> = (0..70_000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        vec![
            ("manifests/a.json".into(), b"{\"x\": 1}".to_vec()),
            ("songs/bin/generic/demo_lead.sng".into(), big),
            ("audio/windows/1.wem".into(), noise),
            ("empty.txt".into(), Vec::new()),
        ]
    }

    #[test]
    fn roundtrip_encrypted_and_plain() {
        for key in [Some(&KEY), None] {
            for compress in [true, false] {
                let files = sample();
                let mut p = Psarc::from_bytes(write_psarc(&files, key, 65536, compress), key).unwrap();
                assert_eq!(p.encrypted_toc, key.is_some());
                assert_eq!(p.entries.len(), files.len());
                for (name, data) in &files {
                    assert_eq!(&p.read(name).unwrap(), data, "{name}");
                }
            }
        }
    }

    #[test]
    fn encrypted_toc_needs_the_key() {
        let bytes = write_psarc(&sample(), Some(&KEY), 65536, true);
        assert!(
            Psarc::from_bytes(bytes.clone(), None)
                .unwrap_err()
                .is_key_missing()
        );
        let wrong = [9u8; 32];
        assert!(Psarc::from_bytes(bytes, Some(&wrong)).is_err());
    }

    #[test]
    fn mac_names_read_as_pc() {
        let files = vec![("songs/bin/macos/x_lead.sng".to_string(), vec![1, 2, 3])];
        let mut p = Psarc::from_bytes(write_psarc(&files, None, 65536, true), None).unwrap();
        assert_eq!(p.platform, Platform::Mac);
        assert_eq!(p.entries[0].name, "songs/bin/generic/x_lead.sng");
        assert_eq!(p.read("songs/bin/macos/X_LEAD.sng").unwrap(), vec![1, 2, 3]);
        assert_eq!(p.find("songs/bin/*.sng").len(), 1);
    }

    #[test]
    fn bad_magic_and_short_file() {
        assert!(Psarc::from_bytes(b"nope".to_vec(), None).is_err());
        assert!(
            Psarc::from_bytes(vec![0u8; 40], None)
                .unwrap_err()
                .to_string()
                .contains("bad magic")
        );
    }

    #[test]
    fn safe_paths() {
        assert_eq!(
            safe_relative_path("../../a/./b:c").unwrap(),
            PathBuf::from("a").join("b_c")
        );
        assert!(safe_relative_path("/..").is_err());
    }
}
