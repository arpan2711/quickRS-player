//! Wwise Vorbis audio (.wem, as Rocksmith 2014 ships it) to a standard Ogg Vorbis file, losslessly.
//! Port of rs-open-format/rsfeedpak/wem.py (itself a port of ww2ogg, BSD-3-Clause, (c) 2002 Xiph.org
//! Foundation, (c) 2009-2016 Adam Gashlin; see assets/wem/LICENSE-ww2ogg.txt, plus revorb's granule pass).
//!
//! Wwise keeps the Vorbis audio packets but stores the three header packets in a compact form (codebooks as ids
//! into a shared library, narrower fields) and strips each audio packet's packet-type and window bits. This
//! rebuilds both and wraps the packets in Ogg pages with correct granule positions. Nothing is decoded or
//! re-encoded, so the audio is exactly what Rocksmith plays. It handles the layouts without a header triad, which
//! covers every Rocksmith 2014 song ("vorb" inside "fmt ", 2-byte packet headers, modified packets, external
//! aoTuV 6.03 codebooks); anything else is an error. The output is byte-identical to the Python port's.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::error::{Error, Result};

const CODEBOOKS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/wem/packed_codebooks_aoTuV_603.bin"
));
/// Kept identical to the Python port's, so outputs can be compared byte for byte.
const VENDOR: &[u8] = b"converted from Audiokinetic Wwise by rsfeedpak (ww2ogg 0.24 port)";
/// Bytes of packet data per Ogg page, as libogg does.
const PAGE_TARGET: usize = 4096;

fn werr(m: impl Into<String>) -> Error {
    Error::Wem(m.into())
}

// --- bit I/O (Vorbis packs bits least-significant first) ----------------------------------------------------
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize, // in bits
}

impl BitReader<'_> {
    fn read(&mut self, n: u32) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        let end = (self.pos + n as usize).div_ceil(8);
        if end > self.data.len() {
            return Err(werr("setup data ended early"));
        }
        let mut v: u64 = 0;
        let start = self.pos >> 3;
        for (k, b) in self.data[start..end].iter().enumerate() {
            v |= (*b as u64) << (8 * k);
        }
        let out = (v >> (self.pos & 7)) & ((1u64 << n) - 1);
        self.pos += n as usize;
        Ok(out as u32)
    }
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    nbits: u32,
}

impl BitWriter {
    fn write(&mut self, value: u32, n: u32) {
        if n == 0 {
            return;
        }
        let v = (value as u64) & ((1u64 << n) - 1);
        self.acc |= v << self.nbits;
        self.nbits += n;
        while self.nbits >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.nbits -= 8;
        }
    }

    /// Append `nbits` bits from a little-endian bit string.
    fn write_bits(&mut self, bytes: &[u8], nbits: usize) {
        let mut left = nbits;
        let mut i = 0;
        while left >= 8 {
            self.write(bytes[i] as u32, 8);
            i += 1;
            left -= 8;
        }
        if left > 0 {
            self.write(bytes[i] as u32, left as u32);
        }
    }

    fn total_bits(&self) -> usize {
        self.out.len() * 8 + self.nbits as usize
    }

    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

fn ilog(v: u32) -> u32 {
    32 - v.leading_zeros()
}

/// Number of values in a lookup-type-1 codebook (from Tremor's _book_maptype1_quantvals).
fn quantvals(entries: u32, dims: u32) -> Result<u32> {
    if dims == 0 {
        return Err(werr("codebook with zero dimensions"));
    }
    let q = (ilog(entries) as i64 - 1) * (dims as i64 - 1);
    let shift = if q >= 0 { q / dims as i64 } else { 0 };
    let mut vals = (entries >> shift.min(31)) as u128;
    let e = entries as u128;
    loop {
        let acc = vals.saturating_pow(dims);
        let acc1 = (vals + 1).saturating_pow(dims);
        if acc <= e && e < acc1 {
            return Ok(vals as u32);
        }
        if acc > e {
            vals -= 1;
        } else {
            vals += 1;
        }
    }
}

// --- codebooks ----------------------------------------------------------------------------------------------
/// One codebook from Wwise's packed form to the Vorbis setup-header form.
fn rebuild_codebook(r: &mut BitReader, w: &mut BitWriter) -> Result<()> {
    let dims = r.read(4)?;
    let entries = r.read(14)?;
    w.write(0x564342, 24); // "BCV"
    w.write(dims, 16);
    w.write(entries, 24);
    let ordered = r.read(1)?;
    w.write(ordered, 1);
    if ordered != 0 {
        w.write(r.read(5)?, 5); // initial length
        let mut cur = 0u32;
        while cur < entries {
            let nb = ilog(entries - cur);
            let num = r.read(nb)?;
            w.write(num, nb);
            cur += num;
        }
        if cur > entries {
            return Err(werr("codebook entry count overflow"));
        }
    } else {
        let length_bits = r.read(3)?;
        let sparse = r.read(1)?;
        if !(1..=5).contains(&length_bits) {
            return Err(werr("nonsense codeword length width"));
        }
        w.write(sparse, 1);
        for _ in 0..entries {
            let mut present = 1;
            if sparse != 0 {
                present = r.read(1)?;
                w.write(present, 1);
            }
            if present != 0 {
                w.write(r.read(length_bits)?, 5);
            }
        }
    }
    let lookup = r.read(1)?;
    w.write(lookup, 4);
    if lookup == 1 {
        w.write(r.read(32)?, 32); // minimum value
        w.write(r.read(32)?, 32); // delta value
        let value_bits = r.read(4)?;
        w.write(value_bits, 4);
        w.write(r.read(1)?, 1); // sequence flag
        for _ in 0..quantvals(entries, dims)? {
            w.write(r.read(value_bits + 1)?, value_bits + 1);
        }
    }
    Ok(())
}

/// ww2ogg's packed codebook file: codebook data, then a table of u32 offsets, then the table's offset.
struct CodebookLibrary {
    offsets: Vec<u32>,
    data: &'static [u8],
    rebuilt: Mutex<HashMap<u32, (Vec<u8>, usize)>>,
}

impl CodebookLibrary {
    fn new(data: &'static [u8]) -> CodebookLibrary {
        let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        let table = u32_at(data.len() - 4) as usize;
        let count = (data.len() - table) / 4;
        let offsets = (0..count).map(|k| u32_at(table + 4 * k)).collect();
        CodebookLibrary {
            offsets,
            data: &data[..table],
            rebuilt: Mutex::new(HashMap::new()),
        }
    }

    fn rebuild(&self, i: u32, w: &mut BitWriter) -> Result<()> {
        let mut cache = self.rebuilt.lock().unwrap();
        if let std::collections::hash_map::Entry::Vacant(slot) = cache.entry(i) {
            if i as usize + 1 >= self.offsets.len() {
                return Err(werr(format!("codebook id {i} is not in the library")));
            }
            let (a, b) = (
                self.offsets[i as usize] as usize,
                self.offsets[i as usize + 1] as usize,
            );
            if a > b || b > self.data.len() {
                return Err(werr(format!("codebook id {i} is not in the library")));
            }
            let raw = &self.data[a..b];
            let mut r = BitReader { data: raw, pos: 0 };
            let mut cw = BitWriter::default();
            rebuild_codebook(&mut r, &mut cw)?;
            if r.pos / 8 + 1 != raw.len() {
                // ww2ogg's check: the packed book is used exactly
                return Err(werr(format!("codebook {i}: size mismatch")));
            }
            let bits = cw.total_bits();
            slot.insert((cw.finish(), bits));
        }
        let (bytes, bits) = &cache[&i];
        w.write_bits(bytes, *bits);
        Ok(())
    }
}

fn codebook_library() -> &'static CodebookLibrary {
    static LIB: OnceLock<CodebookLibrary> = OnceLock::new();
    LIB.get_or_init(|| CodebookLibrary::new(CODEBOOKS))
}

// --- RIFF header --------------------------------------------------------------------------------------------
#[derive(Clone, Debug)]
pub struct WemInfo {
    pub channels: u16,
    pub sample_rate: u32,
    pub avg_bytes_per_second: u32,
    pub sample_count: u32,
    /// Absolute offsets into the file.
    pub setup_offset: usize,
    pub audio_offset: usize,
    pub data_end: usize,
    /// As powers of two.
    pub blocksize_0: u8,
    pub blocksize_1: u8,
    /// Packet-type and window bits stripped from audio packets.
    pub mod_packets: bool,
    /// 2-byte packet headers (else 6: size + granule).
    pub no_granule: bool,
    pub uid: u32,
}

impl WemInfo {
    pub fn duration(&self) -> f64 {
        if self.sample_rate > 0 {
            self.sample_count as f64 / self.sample_rate as f64
        } else {
            0.0
        }
    }
}

fn le32(d: &[u8], at: usize) -> Result<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| werr("truncated header"))
}

fn le16(d: &[u8], at: usize) -> Result<u16> {
    d.get(at..at + 2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| werr("truncated header"))
}

pub fn parse_wem(data: &[u8]) -> Result<WemInfo> {
    if data.len() >= 4 && &data[..4] == b"RIFX" {
        return Err(werr("big-endian (console) .wem isn't supported"));
    }
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(werr("not a RIFF WAVE file"));
    }
    let riff_end = data.len().min(8 + le32(data, 4)? as usize);
    let mut chunks: HashMap<[u8; 4], (usize, usize)> = HashMap::new();
    let mut pos = 12;
    while pos + 8 <= riff_end {
        let size = le32(data, pos + 4)? as usize;
        chunks.insert(data[pos..pos + 4].try_into().unwrap(), (pos + 8, size));
        pos += 8 + size;
    }
    let (Some(&(fmt_off, fmt_size)), Some(&(data_off, data_size))) =
        (chunks.get(b"fmt "), chunks.get(b"data"))
    else {
        return Err(werr("missing fmt or data chunk"));
    };
    let codec = le16(data, fmt_off)?;
    let channels = le16(data, fmt_off + 2)?;
    let rate = le32(data, fmt_off + 4)?;
    let abps = le32(data, fmt_off + 8)?;
    if codec != 0xFFFF {
        return Err(werr(format!("not Wwise Vorbis (codec {codec:#06x})")));
    }
    let (vorb_off, vorb_size): (usize, i64) = if let Some(&(o, s)) = chunks.get(b"vorb") {
        (o, s as i64)
    } else if fmt_size == 0x42 {
        (fmt_off + 0x18, -1)
    } else {
        return Err(werr("no vorb data"));
    };
    let data_end = data_off + data_size;
    if data_end > data.len() {
        return Err(werr("data chunk truncated"));
    }
    let sample_count = le32(data, vorb_off)?;
    let (no_granule, mod_packets, setup, audio, extra) = match vorb_size {
        -1 | 0x2A => {
            let m = !matches!(le32(data, vorb_off + 0x4)?, 0x4A | 0x4B | 0x69 | 0x70);
            (
                true,
                m,
                le32(data, vorb_off + 0x10)?,
                le32(data, vorb_off + 0x14)?,
                vorb_off + 0x24,
            )
        }
        0x32 | 0x34 => (
            false,
            false,
            le32(data, vorb_off + 0x18)?,
            le32(data, vorb_off + 0x1C)?,
            vorb_off + 0x2C,
        ),
        // 0x28 / 0x2C: the old layout with a full header triad, which Rocksmith doesn't use
        _ => {
            return Err(werr(format!(
                "unsupported Wwise Vorbis layout (vorb size {vorb_size:#x})"
            )));
        }
    };
    let bs = data
        .get(extra + 4..extra + 6)
        .ok_or_else(|| werr("truncated header"))?;
    Ok(WemInfo {
        channels,
        sample_rate: rate,
        avg_bytes_per_second: abps,
        sample_count,
        setup_offset: data_off + setup as usize,
        audio_offset: data_off + audio as usize,
        data_end,
        blocksize_0: bs[0],
        blocksize_1: bs[1],
        mod_packets,
        no_granule,
        uid: le32(data, extra)?,
    })
}

// --- Vorbis headers -----------------------------------------------------------------------------------------
fn id_header(info: &WemInfo) -> Vec<u8> {
    let mut h = b"\x01vorbis".to_vec();
    h.extend_from_slice(&0u32.to_le_bytes());
    h.push(info.channels as u8);
    h.extend_from_slice(&info.sample_rate.to_le_bytes());
    h.extend_from_slice(&0i32.to_le_bytes());
    h.extend_from_slice(&((info.avg_bytes_per_second as i64 * 8) as i32).to_le_bytes());
    h.extend_from_slice(&0i32.to_le_bytes());
    h.push((info.blocksize_1 << 4) | info.blocksize_0);
    h.push(1);
    h
}

fn comment_header() -> Vec<u8> {
    let mut h = b"\x03vorbis".to_vec();
    h.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
    h.extend_from_slice(VENDOR);
    h.extend_from_slice(&0u32.to_le_bytes());
    h.push(1);
    h
}

fn check(ok: bool, what: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(werr(format!("invalid setup: {what}")))
    }
}

/// The rebuilt setup packet, each mode's block flag, and the mode-number width in bits.
fn setup_header(data: &[u8], info: &WemInfo, inline_codebooks: bool) -> Result<(Vec<u8>, Vec<bool>, u32)> {
    let hdr = if info.no_granule { 2 } else { 6 };
    let size = le16(data, info.setup_offset)? as usize;
    let start = info.setup_offset + hdr;
    if start + size > info.data_end {
        return Err(werr("setup packet truncated"));
    }
    if start + size != info.audio_offset {
        return Err(werr("first audio packet doesn't follow the setup packet"));
    }
    let mut r = BitReader {
        data: &data[start..start + size],
        pos: 0,
    };
    let mut w = BitWriter::default();
    for b in b"\x05vorbis" {
        w.write(*b as u32, 8);
    }

    let books = r.read(8)? + 1;
    w.write(books - 1, 8);
    if inline_codebooks {
        for _ in 0..books {
            rebuild_codebook(&mut r, &mut w)?;
        }
    } else {
        let lib = codebook_library();
        for _ in 0..books {
            let id = r.read(10)?;
            lib.rebuild(id, &mut w)?;
        }
    }

    w.write(0, 6); // time-domain transforms: one placeholder
    w.write(0, 16);

    let floors = r.read(6)? + 1;
    w.write(floors - 1, 6);
    for _ in 0..floors {
        w.write(1, 16); // floor type 1, the only one Wwise uses
        let partitions = r.read(5)?;
        w.write(partitions, 5);
        let mut classes = Vec::new();
        for _ in 0..partitions {
            let c = r.read(4)?;
            w.write(c, 4);
            classes.push(c);
        }
        let mut dims = Vec::new();
        for _ in 0..classes.iter().max().map(|m| m + 1).unwrap_or(1) {
            let d = r.read(3)?;
            w.write(d, 3);
            dims.push(d + 1);
            let sub = r.read(2)?;
            w.write(sub, 2);
            if sub != 0 {
                let master = r.read(8)?;
                w.write(master, 8);
                check(master < books, "floor masterbook")?;
            }
            for _ in 0..(1u32 << sub) {
                let book = r.read(8)?;
                w.write(book, 8);
                check((book as i64 - 1) < books as i64, "floor subclass book")?;
            }
        }
        w.write(r.read(2)?, 2); // multiplier
        let rangebits = r.read(4)?;
        w.write(rangebits, 4);
        for c in &classes {
            for _ in 0..dims[*c as usize] {
                w.write(r.read(rangebits)?, rangebits);
            }
        }
    }

    let residues = r.read(6)? + 1;
    w.write(residues - 1, 6);
    for _ in 0..residues {
        let rtype = r.read(2)?;
        w.write(rtype, 16);
        check(rtype <= 2, "residue type")?;
        for n in [24, 24, 24] {
            // begin, end, partition size
            w.write(r.read(n)?, n);
        }
        let classifications = r.read(6)? + 1;
        w.write(classifications - 1, 6);
        let classbook = r.read(8)?;
        w.write(classbook, 8);
        check(classbook < books, "residue classbook")?;
        let mut cascade = Vec::new();
        for _ in 0..classifications {
            let low = r.read(3)?;
            w.write(low, 3);
            let flag = r.read(1)?;
            w.write(flag, 1);
            let mut high = 0;
            if flag != 0 {
                high = r.read(5)?;
                w.write(high, 5);
            }
            cascade.push(high * 8 + low);
        }
        for c in cascade {
            for k in 0..8 {
                if c & (1 << k) != 0 {
                    let book = r.read(8)?;
                    w.write(book, 8);
                    check(book < books, "residue book")?;
                }
            }
        }
    }

    let mappings = r.read(6)? + 1;
    w.write(mappings - 1, 6);
    for _ in 0..mappings {
        w.write(0, 16); // mapping type 0
        let mut submaps = 1;
        let flag = r.read(1)?;
        w.write(flag, 1);
        if flag != 0 {
            submaps = r.read(4)? + 1;
            w.write(submaps - 1, 4);
        }
        let flag = r.read(1)?;
        w.write(flag, 1);
        if flag != 0 {
            // square polar coupling
            let steps = r.read(8)? + 1;
            w.write(steps - 1, 8);
            let nb = ilog(info.channels as u32 - 1);
            for _ in 0..steps {
                let mag = r.read(nb)?;
                let ang = r.read(nb)?;
                w.write(mag, nb);
                w.write(ang, nb);
                check(
                    mag != ang && mag < info.channels as u32 && ang < info.channels as u32,
                    "coupling",
                )?;
            }
        }
        let reserved = r.read(2)?;
        w.write(reserved, 2);
        check(reserved == 0, "mapping reserved bits")?;
        if submaps > 1 {
            for _ in 0..info.channels {
                let mux = r.read(4)?;
                w.write(mux, 4);
                check(mux < submaps, "mapping mux")?;
            }
        }
        for _ in 0..submaps {
            w.write(r.read(8)?, 8); // unused time configuration
            let floor = r.read(8)?;
            w.write(floor, 8);
            check(floor < floors, "mapping floor")?;
            let residue = r.read(8)?;
            w.write(residue, 8);
            check(residue < residues, "mapping residue")?;
        }
    }

    let modes = r.read(6)? + 1;
    w.write(modes - 1, 6);
    let mut blockflags = Vec::new();
    for _ in 0..modes {
        let flag = r.read(1)?;
        w.write(flag, 1);
        blockflags.push(flag != 0);
        w.write(0, 16); // window type
        w.write(0, 16); // transform type
        let mapping = r.read(8)?;
        w.write(mapping, 8);
        check(mapping < mappings, "mode mapping")?;
    }
    w.write(1, 1); // framing

    if r.pos.div_ceil(8) != size {
        return Err(werr("setup packet wasn't read exactly"));
    }
    Ok((w.finish(), blockflags, ilog(modes - 1)))
}

// --- audio packets ------------------------------------------------------------------------------------------
/// (packet, block size) for each audio packet, rebuilt into standard Vorbis packets.
fn audio_packets(
    data: &[u8],
    info: &WemInfo,
    blockflags: &[bool],
    mode_bits: u32,
) -> Result<Vec<(Vec<u8>, u32)>> {
    let hdr = if info.no_granule { 2 } else { 6 };
    let mask = (1u32 << mode_bits) - 1;
    let sizes = [1u32 << info.blocksize_0, 1u32 << info.blocksize_1];
    let mut prev_long = false;
    let (mut off, end) = (info.audio_offset, info.data_end);
    let mut out = Vec::new();
    while off < end {
        if off + hdr > end {
            return Err(werr("audio packet header truncated"));
        }
        let size = le16(data, off)? as usize;
        let (start, nxt) = (off + hdr, off + hdr + size);
        if nxt > end {
            return Err(werr("audio packet truncated"));
        }
        off = nxt;
        if size == 0 {
            continue; // carries no audio
        }
        let packet = &data[start..nxt];
        if !info.mod_packets {
            let mode = ((packet[0] >> 1) as u32 & mask) as usize;
            if mode >= blockflags.len() {
                return Err(werr("audio packet with an unknown mode"));
            }
            out.push((packet.to_vec(), sizes[blockflags[mode] as usize]));
            continue;
        }
        let mode = (packet[0] as u32 & mask) as usize;
        if mode >= blockflags.len() {
            return Err(werr("audio packet with an unknown mode"));
        }
        let (head, shift) = if blockflags[mode] {
            // long window: rebuild the previous/next window flags
            let mut next_long = false;
            if nxt + hdr < end && le16(data, nxt)? > 0 {
                let next_mode = (data[nxt + hdr] as u32 & mask) as usize;
                next_long = next_mode < blockflags.len() && blockflags[next_mode];
            }
            let h = ((mode as u32) << 1)
                | ((prev_long as u32) << (1 + mode_bits))
                | ((next_long as u32) << (2 + mode_bits));
            (h, 3 + mode_bits)
        } else {
            ((mode as u32) << 1, 1 + mode_bits)
        };
        // drop the packet's low mode_bits bits, shift the rest up by `shift` and put the header bits below
        let mut w = BitWriter::default();
        w.write(head, shift);
        let mut r = BitReader {
            data: packet,
            pos: mode_bits as usize,
        };
        let mut left = 8 * size - mode_bits as usize;
        while left > 0 {
            let n = left.min(24) as u32;
            let v = r.read(n)?;
            w.write(v, n);
            left -= n as usize;
        }
        out.push((w.finish(), sizes[blockflags[mode] as usize]));
        prev_long = blockflags[mode];
    }
    Ok(out)
}

// --- Ogg ----------------------------------------------------------------------------------------------------
fn crc_table() -> &'static [u32; 256] {
    static T: OnceLock<[u32; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 {
                    (r << 1) ^ 0x04C1_1DB7
                } else {
                    r << 1
                };
            }
            *e = r;
        }
        t
    })
}

/// Ogg's CRC-32 (polynomial 0x04C11DB7, MSB first, no reflection, init 0).
pub fn ogg_crc(page: &[u8]) -> u32 {
    let t = crc_table();
    page.iter()
        .fold(0u32, |crc, b| (crc << 8) ^ t[((crc >> 24) as u8 ^ b) as usize])
}

struct OggWriter {
    serial: u32,
    out: Vec<u8>,
    seq: u32,
    laces: Vec<u8>,
    body: Vec<u8>,
    /// Of the last packet that ends on the current page.
    granule: i64,
    /// The current page starts mid-packet.
    continued: bool,
    bos: bool,
}

impl OggWriter {
    fn new(serial: u32) -> OggWriter {
        OggWriter {
            serial,
            out: Vec::new(),
            seq: 0,
            laces: Vec::new(),
            body: Vec::new(),
            granule: -1,
            continued: false,
            bos: true,
        }
    }

    fn add(&mut self, packet: &[u8], granule: i64) {
        let n = packet.len();
        let count = n / 255 + 1;
        for i in 0..count {
            if self.laces.len() == 255 {
                // page full: start a new one (continuing this packet if it began here)
                self.emit(false);
                self.continued = i > 0;
            }
            let lace = if i < count - 1 { 255 } else { n % 255 };
            self.laces.push(lace as u8);
            self.body.extend_from_slice(&packet[i * 255..i * 255 + lace]);
        }
        self.granule = granule;
    }

    fn flush(&mut self, eos: bool) {
        if !self.laces.is_empty() || eos {
            self.emit(eos);
        }
    }

    fn emit(&mut self, eos: bool) {
        let flags = (self.continued as u8) | ((self.bos as u8) << 1) | ((eos as u8) << 2);
        let mut page = Vec::with_capacity(27 + self.laces.len() + self.body.len());
        page.extend_from_slice(b"OggS");
        page.push(0);
        page.push(flags);
        page.extend_from_slice(&self.granule.to_le_bytes());
        page.extend_from_slice(&self.serial.to_le_bytes());
        page.extend_from_slice(&self.seq.to_le_bytes());
        page.extend_from_slice(&0u32.to_le_bytes());
        page.push(self.laces.len() as u8);
        page.extend_from_slice(&self.laces);
        page.extend_from_slice(&self.body);
        let crc = ogg_crc(&page);
        page[22..26].copy_from_slice(&crc.to_le_bytes());
        self.out.extend_from_slice(&page);
        self.seq += 1;
        self.bos = false;
        self.continued = false;
        self.laces.clear();
        self.body.clear();
        self.granule = -1;
    }
}

/// Convert a Wwise Vorbis .wem to an Ogg Vorbis file without re-encoding.
pub fn wem_to_ogg(data: &[u8]) -> Result<Vec<u8>> {
    let info = parse_wem(data)?;
    let (setup, blockflags, mode_bits) = match setup_header(data, &info, false) {
        Ok(x) => x,
        // some Wwise versions store the codebooks inline instead of by id
        Err(external) => setup_header(data, &info, true).map_err(|_| external)?,
    };
    let packets = audio_packets(data, &info, &blockflags, mode_bits)?;
    if packets.is_empty() {
        return Err(werr("no audio packets"));
    }

    // Granule positions (revorb): a packet decodes to a quarter of the previous block plus a quarter of its own.
    let mut granules = Vec::with_capacity(packets.len());
    let (mut total, mut prev) = (0i64, 0u32);
    for (_, bs) in &packets {
        if prev != 0 {
            total += ((prev + bs) / 4) as i64;
        }
        prev = *bs;
        granules.push(total);
    }
    // Trim the end to the exact length Wwise declares (decoders drop samples past the last granule).
    let n = granules.len();
    let sc = info.sample_count as i64;
    if n > 1 && granules[n - 2] <= sc && sc < granules[n - 1] {
        granules[n - 1] = sc;
    }

    let mut ogg = OggWriter::new(if info.uid != 0 { info.uid } else { 0x52534650 });
    ogg.add(&id_header(&info), 0);
    ogg.flush(false); // the identification header sits alone on the first page
    ogg.add(&comment_header(), 0);
    ogg.add(&setup, 0);
    ogg.flush(false); // audio starts on a fresh page
    let last = packets.len() - 1;
    for (i, (packet, _)) in packets.iter().enumerate() {
        ogg.add(packet, granules[i]);
        if i < last && ogg.body.len() >= PAGE_TARGET {
            ogg.flush(false);
        }
    }
    ogg.flush(true);
    Ok(ogg.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ogg_crc_known_value() {
        // CRC of "OggS" page header bytes with zeros, checked against libogg's table-driven CRC
        assert_eq!(ogg_crc(b""), 0);
        assert_eq!(ogg_crc(&[0x80]), crc_table()[0x80]);
    }

    #[test]
    fn quantvals_matches_tremor() {
        assert_eq!(quantvals(256, 2).unwrap(), 16);
        assert_eq!(quantvals(81, 4).unwrap(), 3);
        assert_eq!(quantvals(100, 1).unwrap(), 100);
        assert_eq!(quantvals(80, 4).unwrap(), 2);
    }

    #[test]
    fn library_loads() {
        let lib = codebook_library();
        assert!(lib.offsets.len() > 500);
        let mut w = BitWriter::default();
        lib.rebuild(0, &mut w).unwrap();
        assert!(w.total_bits() > 24);
    }

    #[test]
    fn rejects_non_wem() {
        assert!(wem_to_ogg(b"RIFF\0\0\0\0WAVEfmt ").is_err());
        assert!(wem_to_ogg(b"nope").is_err());
    }
}
