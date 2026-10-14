//! SNG note chart decryption and parsing (Rocksmith 2014). Port of rsplay/sng.py plus the extra records of
//! rsfeedpak/sngfull.py (every level, bend curves, tones, vocal pitches) and its PC/Mac key handling.
//!
//! Packed file:   u32 LE magic 0x4A, u32 LE flags, 16-byte IV, then AES-256-CTR data
//!                (128-bit big-endian counter starting at the IV), then a 56-byte signature.
//! Decrypted:     u32 LE uncompressed size, then a zlib stream.
//! Decompressed:  little-endian sections, each a u32 count followed by records.
//! Layouts verified (in the Python original) against Rocksmith2014.NET (src/Rocksmith2014.SNG/Types).

use crate::crypto;
use crate::error::{Error, Result};
use crate::keys::Keys;
use crate::model::{Anchor, Arrangement, Beat, Chord, HandShape, Note, Section, Techs, ToneChange};
use crate::psarc::{Platform, inflate};

pub const SNG_MAGIC: u32 = 0x4A;

// --- note mask bits ---------------------------------------------------------------------------------------
pub const MASK_CHORD: u32 = 0x2;
pub const MASK_OPEN: u32 = 0x4;
/// Fret-hand mute on chords.
pub const MASK_FRET_HAND_MUTE: u32 = 0x8;
pub const MASK_TREMOLO: u32 = 0x10;
pub const MASK_HARMONIC: u32 = 0x20;
pub const MASK_PALM_MUTE: u32 = 0x40;
pub const MASK_SLAP: u32 = 0x80;
pub const MASK_PLUCK: u32 = 0x100;
pub const MASK_HAMMER_ON: u32 = 0x200;
pub const MASK_PULL_OFF: u32 = 0x400;
pub const MASK_SLIDE: u32 = 0x800;
pub const MASK_BEND: u32 = 0x1000;
pub const MASK_SUSTAIN: u32 = 0x2000;
pub const MASK_TAP: u32 = 0x4000;
pub const MASK_PINCH_HARMONIC: u32 = 0x8000;
pub const MASK_VIBRATO: u32 = 0x10000;
/// Fret-hand mute on single notes and chord notes.
pub const MASK_MUTE: u32 = 0x20000;
pub const MASK_IGNORE: u32 = 0x40000;
pub const MASK_HIGH_DENSITY: u32 = 0x200000;
pub const MASK_UNPITCHED_SLIDE: u32 = 0x400000;
pub const MASK_CHORD_NOTES: u32 = 0x1000000;
pub const MASK_ACCENT: u32 = 0x4000000;
/// linkNext
pub const MASK_PARENT: u32 = 0x8000000;
pub const MASK_CHORD_PANEL: u32 = 0x80000000;

pub const BEAT_FIRST_OF_MEASURE: i32 = 0x1;

const NOTE_TECHNIQUE_BITS: [(u32, Techs); 18] = [
    (MASK_HAMMER_ON, Techs::HAMMER_ON),
    (MASK_PULL_OFF, Techs::PULL_OFF),
    (MASK_PALM_MUTE, Techs::PALM_MUTE),
    (MASK_MUTE, Techs::FRET_HAND_MUTE),
    (MASK_HARMONIC, Techs::HARMONIC),
    (MASK_PINCH_HARMONIC, Techs::PINCH_HARMONIC),
    (MASK_TAP, Techs::TAP),
    (MASK_TREMOLO, Techs::TREMOLO),
    (MASK_SLAP, Techs::SLAP),
    (MASK_PLUCK, Techs::POP),
    (MASK_ACCENT, Techs::ACCENT),
    (MASK_PARENT, Techs::LINK_NEXT),
    (MASK_IGNORE, Techs::IGNORE),
    (MASK_BEND, Techs::BEND),
    (MASK_VIBRATO, Techs::VIBRATO),
    (MASK_SLIDE, Techs::SLIDE),
    (MASK_UNPITCHED_SLIDE, Techs::UNPITCHED_SLIDE),
    (MASK_SUSTAIN, Techs::SUSTAIN),
];
const CHORD_TECHNIQUE_BITS: [(u32, Techs); 5] = [
    (MASK_PALM_MUTE, Techs::PALM_MUTE),
    (MASK_FRET_HAND_MUTE, Techs::FRET_HAND_MUTE),
    (MASK_ACCENT, Techs::ACCENT),
    (MASK_PARENT, Techs::LINK_NEXT),
    (MASK_IGNORE, Techs::IGNORE),
];

fn techniques(mask: u32, table: &[(u32, Techs)]) -> Techs {
    table
        .iter()
        .filter(|(bit, _)| mask & bit != 0)
        .fold(Techs::NONE, |acc, (_, t)| acc | *t)
}

// --- record sizes -----------------------------------------------------------------------------------------
const BEAT_SIZE: usize = 16;
const PHRASE_SIZE: usize = 44;
const CHORD_SIZE: usize = 72;
const BEND_VALUE_SIZE: usize = 12;
const BEND_DATA32_SIZE: usize = 32 * BEND_VALUE_SIZE + 4; // 388
const CHORD_NOTES_SIZE: usize = 24 + 6 * BEND_DATA32_SIZE + 6 + 6 + 12; // 2376
const VOCAL_SIZE: usize = 60;
const SYMBOLS_HEADER_SIZE: usize = 32;
const SYMBOLS_TEXTURE_SIZE: usize = 144;
const SYMBOL_DEFINITION_SIZE: usize = 44;
const PHRASE_ITERATION_SIZE: usize = 24;
const PHRASE_EXTRA_INFO_SIZE: usize = 16;
const ACTION_SIZE: usize = 260;
const EVENT_SIZE: usize = 260;
const TONE_SIZE: usize = 8;
const DNA_SIZE: usize = 8;
const SECTION_SIZE: usize = 88;
const ANCHOR_SIZE: usize = 28;
const ANCHOR_EXTENSION_SIZE: usize = 12;
const FINGERPRINT_SIZE: usize = 20;
const NOTE_SIZE: usize = 63;

// --- parsed structures ------------------------------------------------------------------------------------
#[derive(Clone, Debug)]
pub struct SngBeat {
    pub time: f32,
    pub measure: i16,
    pub beat: i16,
    pub mask: i32,
}

#[derive(Clone, Debug)]
pub struct SngPhrase {
    pub max_difficulty: i32,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct SngChordTemplate {
    pub mask: u32,
    pub frets: [i8; 6],
    pub fingers: [i8; 6],
    pub midi: [i32; 6],
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct SngChordNotes {
    pub masks: [u32; 6],
    /// Per string: (time, step) points.
    pub bends: [Vec<(f32, f32)>; 6],
    pub slide_to: [i8; 6],
    pub slide_unpitch_to: [i8; 6],
    pub vibrato: [i16; 6],
}

impl SngChordNotes {
    pub fn max_bend(&self, s: usize) -> f32 {
        self.bends[s].iter().map(|b| b.1).fold(0.0, f32::max)
    }
}

#[derive(Clone, Debug)]
pub struct SngPhraseIteration {
    pub phrase_id: i32,
    pub start: f32,
    pub end: f32,
}

#[derive(Clone, Debug)]
pub struct SngSection {
    pub name: String,
    pub number: i32,
    pub start: f32,
    pub end: f32,
}

#[derive(Clone, Debug)]
pub struct SngNote {
    pub mask: u32,
    pub time: f32,
    pub string: i8,
    pub fret: i8,
    pub chord_id: i32,
    pub chord_notes_id: i32,
    pub phrase_iteration_id: i32,
    pub slide_to: i8,
    pub slide_unpitch_to: i8,
    pub left_hand: i8,
    pub tap: i8,
    pub pick_direction: i8,
    pub slap: i8,
    pub pluck: i8,
    pub vibrato: i16,
    pub sustain: f32,
    pub max_bend: f32,
    pub bends: Vec<(f32, f32)>,
}

impl SngNote {
    pub fn is_chord(&self) -> bool {
        self.chord_id != -1
    }
}

/// (chord template id, start, end, arpeggio)
#[derive(Clone, Debug)]
pub struct SngShape {
    pub chord_id: i32,
    pub start: f32,
    pub end: f32,
    pub arpeggio: bool,
}

#[derive(Clone, Debug)]
pub struct SngAnchor {
    pub start: f32,
    pub end: f32,
    pub fret: u8,
    pub width: i32,
}

#[derive(Clone, Debug)]
pub struct SngLevel {
    pub difficulty: i32,
    pub anchors: Vec<SngAnchor>,
    pub handshapes: Vec<SngShape>,
    pub notes: Vec<SngNote>,
}

#[derive(Clone, Debug, Default)]
pub struct SngMetadata {
    pub capo: i32,
    pub song_length: f32,
    pub tuning: Vec<i16>,
    pub max_difficulty: i32,
    pub start_time: f32,
    pub first_note_time: f32,
}

#[derive(Clone, Debug)]
pub struct SngVocal {
    pub time: f32,
    /// MIDI pitch; 254/255 = unpitched.
    pub note: i32,
    pub length: f32,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Sng {
    pub beats: Vec<SngBeat>,
    pub phrases: Vec<SngPhrase>,
    pub chord_templates: Vec<SngChordTemplate>,
    pub chord_notes: Vec<SngChordNotes>,
    pub vocals: Vec<SngVocal>,
    pub phrase_iterations: Vec<SngPhraseIteration>,
    /// (time, tone id 0..3 = Tone_A..Tone_D)
    pub tones: Vec<(f32, i32)>,
    pub sections: Vec<SngSection>,
    pub levels: Vec<SngLevel>,
    pub metadata: SngMetadata,
}

// --- decryption -------------------------------------------------------------------------------------------
pub fn is_packed_sng(data: &[u8]) -> bool {
    data.len() >= 24 && u32::from_le_bytes([data[0], data[1], data[2], data[3]]) == SNG_MAGIC
}

/// Decrypt and decompress a packed .sng file, returning the plain SNG bytes.
pub fn decrypt_sng(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    if data.len() < 24 {
        return Err(Error::Sng("file too short to be an SNG".into()));
    }
    let magic = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if magic != SNG_MAGIC {
        return Err(Error::Sng(format!(
            "not a packed SNG (magic {magic:#x}, expected {SNG_MAGIC:#x})"
        )));
    }
    let iv: [u8; 16] = data[8..24].try_into().unwrap();
    let plain = crypto::ctr_apply(&data[24..], key, &iv);
    if plain.len() < 4 {
        return Err(Error::Sng("SNG too short".into()));
    }
    let expected = u32::from_le_bytes(plain[0..4].try_into().unwrap()) as usize;
    let out = inflate(&plain[4..])
        .map_err(|_| Error::Sng("SNG decompression failed (wrong SNG key, or not a PC file?)".into()))?;
    if out.len() != expected {
        return Err(Error::Sng(format!(
            "SNG size mismatch: header says {expected} bytes, got {}",
            out.len()
        )));
    }
    Ok(out)
}

/// Encrypt a plain SNG the way Rocksmith packs it: magic, flags 3, IV, AES-CTR(len + zlib), 56 unused bytes.
pub fn pack_sng(plain: &[u8], key: &[u8; 32], iv: &[u8; 16]) -> Vec<u8> {
    let mut payload = (plain.len() as u32).to_le_bytes().to_vec();
    payload.extend(crate::psarc::deflate(plain));
    let mut out = Vec::with_capacity(payload.len() + 80);
    out.extend_from_slice(&SNG_MAGIC.to_le_bytes());
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(iv);
    out.extend(crypto::ctr_apply(&payload, key, iv));
    out.extend_from_slice(&[0u8; 56]);
    out
}

/// A chart as plain SNG bytes, whichever platform's key it was packed with (the package's own key first).
pub fn decrypt_chart(data: &[u8], keys: &Keys, platform: Platform) -> Result<Vec<u8>> {
    if !is_packed_sng(data) {
        return Ok(data.to_vec());
    }
    let order = match platform {
        Platform::Mac => [("mac", keys.sng_mac), ("pc", keys.sng)],
        Platform::Pc => [("pc", keys.sng), ("mac", keys.sng_mac)],
    };
    let mut errors = Vec::new();
    for (name, key) in order {
        let Some(key) = key else { continue };
        match decrypt_sng(data, &key) {
            Ok(p) => return Ok(p),
            Err(e) => errors.push(format!("{name} key: {e}")),
        }
    }
    // A plain SNG can start like a packed one (a chart with 74 beats).
    if parse_sng(data).is_ok() {
        return Ok(data.to_vec());
    }
    if keys.sng.is_none() && keys.sng_mac.is_none() {
        return Err(Error::KeyMissing(
            "SNG note charts are encrypted and no SNG key is configured. \
             Set it in keys.json (see keys.example.json) or via RSPLAY_SNG_KEY."
                .into(),
        ));
    }
    if platform == Platform::Mac && keys.sng_mac.is_none() {
        return Err(Error::KeyMissing(
            "This is a Mac package and its charts need the Mac SNG key. Add \"sng_mac_key\" to keys.json \
             or set RSPLAY_SNG_MAC_KEY."
                .into(),
        ));
    }
    Err(Error::Sng(format!(
        "chart decryption failed ({})",
        errors.join("; ")
    )))
}

/// Parse an SNG that may be packed (encrypted) or plain.
pub fn load_sng(data: &[u8], keys: &Keys, platform: Platform) -> Result<Sng> {
    parse_sng(&decrypt_chart(data, keys, platform)?)
}

// --- parsing ----------------------------------------------------------------------------------------------
struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn need(&self, n: usize) -> Result<()> {
        if self.pos + n > self.d.len() {
            Err(Error::Sng(format!(
                "unexpected end of SNG data at offset {}",
                self.pos
            )))
        } else {
            Ok(())
        }
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.need(n)?;
        let s = &self.d[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    fn i8(&mut self) -> Result<i8> {
        Ok(self.bytes(1)?[0] as i8)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    fn cstr(&mut self, n: usize) -> Result<String> {
        Ok(cstr(self.bytes(n)?))
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }
    /// A record count, checked against what's left.
    fn count(&mut self, record_size: usize, what: &str) -> Result<usize> {
        let n = self.u32()? as usize;
        if n.saturating_mul(record_size) > self.d.len() - self.pos {
            return Err(Error::Sng(format!(
                "{what}: count {n} exceeds remaining data at offset {}",
                self.pos - 4
            )));
        }
        Ok(n)
    }
    fn skip_records(&mut self, record_size: usize, what: &str) -> Result<usize> {
        let n = self.count(record_size, what)?;
        self.pos += n * record_size;
        Ok(n)
    }
}

fn cstr(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

fn i8x6(b: &[u8]) -> [i8; 6] {
    core::array::from_fn(|i| b[i] as i8)
}

/// Parse decrypted, decompressed SNG bytes.
pub fn parse_sng(data: &[u8]) -> Result<Sng> {
    let mut r = Reader { d: data, pos: 0 };
    let mut sng = Sng::default();

    for _ in 0..r.count(BEAT_SIZE, "beats")? {
        let time = r.f32()?;
        let measure = r.i16()?;
        let beat = r.i16()?;
        let _pi = r.i32()?;
        let mask = r.i32()?;
        sng.beats.push(SngBeat {
            time,
            measure,
            beat,
            mask,
        });
    }
    for _ in 0..r.count(PHRASE_SIZE, "phrases")? {
        r.skip(4)?; // solo, disparity, ignore, padding
        let max_difficulty = r.i32()?;
        let _links = r.i32()?;
        let name = r.cstr(32)?;
        sng.phrases.push(SngPhrase { max_difficulty, name });
    }
    for _ in 0..r.count(CHORD_SIZE, "chord templates")? {
        let mask = r.u32()?;
        let frets = i8x6(r.bytes(6)?);
        let fingers = i8x6(r.bytes(6)?);
        let mut midi = [0i32; 6];
        for m in &mut midi {
            *m = r.i32()?;
        }
        let name = r.cstr(32)?;
        sng.chord_templates.push(SngChordTemplate {
            mask,
            frets,
            fingers,
            midi,
            name,
        });
    }
    for _ in 0..r.count(CHORD_NOTES_SIZE, "chord notes")? {
        sng.chord_notes.push(read_chord_notes(&mut r)?);
    }
    let vocal_count = r.count(VOCAL_SIZE, "vocals")?;
    for _ in 0..vocal_count {
        let time = r.f32()?;
        let note = r.i32()?;
        let length = r.f32()?;
        let text = r.cstr(48)?;
        sng.vocals.push(SngVocal {
            time,
            note,
            length,
            text,
        });
    }
    if vocal_count > 0 {
        r.skip_records(SYMBOLS_HEADER_SIZE, "symbol headers")?;
        r.skip_records(SYMBOLS_TEXTURE_SIZE, "symbol textures")?;
        r.skip_records(SYMBOL_DEFINITION_SIZE, "symbol definitions")?;
    }
    for _ in 0..r.count(PHRASE_ITERATION_SIZE, "phrase iterations")? {
        let phrase_id = r.i32()?;
        let start = r.f32()?;
        let end = r.f32()?;
        r.skip(12)?;
        sng.phrase_iterations.push(SngPhraseIteration {
            phrase_id,
            start,
            end,
        });
    }
    r.skip_records(PHRASE_EXTRA_INFO_SIZE, "phrase extra info")?;
    for _ in 0..r.count(8, "linked difficulties")? {
        r.u32()?; // level break
        r.skip_records(4, "linked difficulty phrases")?;
    }
    r.skip_records(ACTION_SIZE, "actions")?;
    r.skip_records(EVENT_SIZE, "events")?;
    for _ in 0..r.count(TONE_SIZE, "tones")? {
        let t = r.f32()?;
        let id = r.i32()?;
        sng.tones.push((t, id));
    }
    r.skip_records(DNA_SIZE, "DNAs")?;
    for _ in 0..r.count(SECTION_SIZE, "sections")? {
        let name = r.cstr(32)?;
        let number = r.i32()?;
        let start = r.f32()?;
        let end = r.f32()?;
        r.skip(8 + 36)?;
        sng.sections.push(SngSection {
            name,
            number,
            start,
            end,
        });
    }
    for _ in 0..r.count(4, "levels")? {
        sng.levels.push(read_level(&mut r)?);
    }

    // metadata: 4 doubles, first beat length, start time, capo (i8), date (32), part (i16), song length
    for _ in 0..4 {
        r.f64()?;
    }
    let _first_beat_len = r.f32()?;
    let start_time = r.f32()?;
    let capo = r.i8()? as i32;
    r.skip(32)?;
    let _part = r.i16()?;
    let song_length = r.f32()?;
    let mut tuning = Vec::new();
    for _ in 0..r.count(2, "tuning")? {
        tuning.push(r.i16()?);
    }
    r.f32()?;
    let first_note_time = r.f32()?;
    let max_difficulty = r.i32()?;
    sng.metadata = SngMetadata {
        capo: capo.max(0),
        song_length,
        tuning,
        max_difficulty,
        start_time,
        first_note_time,
    };
    Ok(sng)
}

fn read_bends(r: &mut Reader, n: usize) -> Result<Vec<(f32, f32)>> {
    let mut pts = Vec::with_capacity(n);
    for _ in 0..n {
        let t = r.f32()?;
        let step = r.f32()?;
        r.skip(4)?;
        pts.push((t, step));
    }
    Ok(pts)
}

fn read_chord_notes(r: &mut Reader) -> Result<SngChordNotes> {
    r.need(CHORD_NOTES_SIZE)?;
    let start = r.pos;
    let mut masks = [0u32; 6];
    for m in &mut masks {
        *m = r.u32()?;
    }
    let mut bends: [Vec<(f32, f32)>; 6] = Default::default();
    for b in &mut bends {
        let base = r.pos;
        r.pos = base + 32 * BEND_VALUE_SIZE;
        let used = r.i32()?.clamp(0, 32) as usize;
        r.pos = base;
        *b = read_bends(r, used)?;
        r.pos = base + BEND_DATA32_SIZE;
    }
    let slide_to = i8x6(r.bytes(6)?);
    let slide_unpitch_to = i8x6(r.bytes(6)?);
    let mut vibrato = [0i16; 6];
    for v in &mut vibrato {
        *v = r.i16()?;
    }
    r.pos = start + CHORD_NOTES_SIZE;
    Ok(SngChordNotes {
        masks,
        bends,
        slide_to,
        slide_unpitch_to,
        vibrato,
    })
}

fn read_level(r: &mut Reader) -> Result<SngLevel> {
    let difficulty = r.i32()?;
    let mut anchors = Vec::new();
    for _ in 0..r.count(ANCHOR_SIZE, "anchors")? {
        let start = r.f32()?;
        let end = r.f32()?;
        r.skip(8)?; // first / last note time
        let fret = r.u8()?;
        r.skip(3)?;
        let width = r.i32()?;
        let _pi = r.i32()?;
        anchors.push(SngAnchor {
            start,
            end,
            fret,
            width,
        });
    }
    r.skip_records(ANCHOR_EXTENSION_SIZE, "anchor extensions")?;
    let mut shapes = Vec::new();
    for arpeggio in [false, true] {
        // "hand shapes", then "arpeggios" (shapes held while picking through)
        let what = if arpeggio { "arpeggios" } else { "hand shapes" };
        for _ in 0..r.count(FINGERPRINT_SIZE, what)? {
            let chord_id = r.i32()?;
            let start = r.f32()?;
            let end = r.f32()?;
            r.skip(8)?;
            shapes.push(SngShape {
                chord_id,
                start,
                end,
                arpeggio,
            });
        }
    }
    // Stable sort: at equal starts, hand shapes before arpeggios (as Python's sorted()).
    shapes.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut notes = Vec::new();
    for _ in 0..r.count(NOTE_SIZE, "notes")? {
        let mask = r.u32()?;
        r.skip(8)?; // flags, hash
        let time = r.f32()?;
        let string = r.i8()?;
        let fret = r.i8()?;
        r.skip(2)?; // anchor fret / width
        let chord_id = r.i32()?;
        let chord_notes_id = r.i32()?;
        let _phrase_id = r.i32()?;
        let phrase_iteration_id = r.i32()?;
        r.skip(2 * 2 + 2 * 3)?; // fingerprints, next, prev, parent
        let slide_to = r.i8()?;
        let slide_unpitch_to = r.i8()?;
        let left_hand = r.i8()?;
        let tap = r.i8()?;
        let pick_direction = r.i8()?;
        let slap = r.i8()?;
        let pluck = r.i8()?;
        let vibrato = r.i16()?;
        let sustain = r.f32()?;
        let max_bend = r.f32()?;
        let n = r.count(BEND_VALUE_SIZE, "bend values")?;
        let bends = read_bends(r, n)?;
        notes.push(SngNote {
            mask,
            time,
            string,
            fret,
            chord_id,
            chord_notes_id,
            phrase_iteration_id,
            slide_to,
            slide_unpitch_to,
            left_hand,
            tap,
            pick_direction,
            slap,
            pluck,
            vibrato,
            sustain,
            max_bend,
            bends,
        });
    }
    r.skip_records(4, "average notes per iteration")?;
    r.skip_records(4, "notes in iteration (excl. ignored)")?;
    r.skip_records(4, "notes in iteration (all)")?;
    Ok(SngLevel {
        difficulty,
        anchors,
        handshapes: shapes,
        notes,
    })
}

// --- difficulty -------------------------------------------------------------------------------------------
/// Python's round() for the level pick: round half to even.
pub fn py_round(x: f64) -> i32 {
    x.round_ties_even() as i32
}

impl Sng {
    fn level_for(&self, max_diff: i32) -> &SngLevel {
        for d in (0..=max_diff).rev() {
            if let Some(l) = self.levels.iter().find(|l| l.difficulty == d) {
                return l;
            }
        }
        &self.levels[0]
    }

    fn hardest(&self) -> &SngLevel {
        // max() returns the first of equal maxima in Python
        let mut best = &self.levels[0];
        for l in &self.levels {
            if l.difficulty > best.difficulty {
                best = l;
            }
        }
        best
    }

    /// (level, lo, hi) for each phrase iteration at a mastery between 0 and 1: like Rocksmith, each phrase uses
    /// the level that is that fraction of its own maximum difficulty (so hard phrases lose the most).
    pub fn phrase_levels(&self, mastery: f64) -> Vec<(&SngLevel, f64, f64)> {
        let pis = &self.phrase_iterations;
        let m = mastery.clamp(0.0, 1.0);
        pis.iter()
            .enumerate()
            .map(|(i, pi)| {
                let lo = if i == 0 {
                    f64::NEG_INFINITY
                } else {
                    pi.start as f64
                };
                let hi = pis.get(i + 1).map(|n| n.start as f64).unwrap_or(f64::INFINITY);
                let max_diff = usize::try_from(pi.phrase_id)
                    .ok()
                    .and_then(|k| self.phrases.get(k))
                    .map(|p| p.max_difficulty)
                    .unwrap_or(0);
                (self.level_for(py_round(m * max_diff as f64)), lo, hi)
            })
            .collect()
    }

    /// Notes at a mastery between 0 and 1.
    pub fn notes_at_mastery(&self, mastery: f64) -> Vec<&SngNote> {
        if self.levels.is_empty() {
            return Vec::new();
        }
        if self.phrase_iterations.is_empty() {
            return self.hardest().notes.iter().collect();
        }
        let mut out = Vec::new();
        for (level, lo, hi) in self.phrase_levels(mastery) {
            let a = level.notes.partition_point(|n| (n.time as f64) < lo);
            let b = level.notes.partition_point(|n| (n.time as f64) < hi);
            out.extend(level.notes[a..b.max(a)].iter());
        }
        out
    }

    pub fn handshapes_at_mastery(&self, mastery: f64) -> Vec<&SngShape> {
        if self.levels.is_empty() {
            return Vec::new();
        }
        if self.phrase_iterations.is_empty() {
            return self.hardest().handshapes.iter().collect();
        }
        let mut out = Vec::new();
        for (level, lo, hi) in self.phrase_levels(mastery) {
            out.extend(
                level
                    .handshapes
                    .iter()
                    .filter(|h| lo <= h.start as f64 && (h.start as f64) < hi),
            );
        }
        out
    }

    pub fn anchors_at_mastery(&self, mastery: f64) -> Vec<&SngAnchor> {
        if self.levels.is_empty() {
            return Vec::new();
        }
        if self.phrase_iterations.is_empty() {
            return self.hardest().anchors.iter().collect();
        }
        let mut out = Vec::new();
        for (level, lo, hi) in self.phrase_levels(mastery) {
            out.extend(
                level
                    .anchors
                    .iter()
                    .filter(|a| lo <= a.start as f64 && (a.start as f64) < hi),
            );
        }
        out
    }

    pub fn has_levels(&self) -> bool {
        self.levels.len() > 1
    }
}

// --- conversion to the model ------------------------------------------------------------------------------
fn bend_curve(points: &[(f32, f32)]) -> Vec<(f64, f64)> {
    points.iter().map(|&(t, s)| (t as f64, s as f64)).collect()
}

fn note_to_model(n: &SngNote) -> Note {
    let mut t = techniques(n.mask, &NOTE_TECHNIQUE_BITS);
    t = t
        .with(Techs::SLIDE, n.slide_to >= 0)
        .with(Techs::UNPITCHED_SLIDE, n.slide_unpitch_to >= 0)
        .with(Techs::BEND, n.max_bend > 0.0)
        .with(Techs::VIBRATO, n.vibrato > 0)
        .with(Techs::TAP, n.tap > 0);
    Note {
        time: n.time as f64,
        string: n.string as i32,
        fret: n.fret as i32,
        sustain: (n.sustain as f64).max(0.0),
        techniques: t,
        slide_to: n.slide_to as i32,
        slide_unpitch_to: n.slide_unpitch_to as i32,
        bend: (n.max_bend as f64).max(0.0),
        bend_curve: bend_curve(&n.bends),
    }
}

fn chord_to_model(sng: &Sng, n: &SngNote) -> Result<Chord> {
    let template = usize::try_from(n.chord_id)
        .ok()
        .and_then(|k| sng.chord_templates.get(k))
        .ok_or_else(|| {
            Error::Sng(format!(
                "chord at {:.3}s references missing template {}",
                n.time, n.chord_id
            ))
        })?;
    let chord_techs = techniques(n.mask, &CHORD_TECHNIQUE_BITS);
    let cn = usize::try_from(n.chord_notes_id)
        .ok()
        .and_then(|k| sng.chord_notes.get(k));
    let mut notes = Vec::new();
    for s in 0..6 {
        let fret = template.frets[s] as i32;
        if fret < 0 {
            continue;
        }
        let mut techs = chord_techs - (Techs::LINK_NEXT | Techs::ACCENT | Techs::IGNORE);
        let (mut slide_to, mut slide_unpitch_to, mut bend, mut curve) = (-1, -1, 0.0, Vec::new());
        if let Some(cn) = cn {
            techs |= techniques(cn.masks[s], &NOTE_TECHNIQUE_BITS);
            slide_to = cn.slide_to[s] as i32;
            slide_unpitch_to = cn.slide_unpitch_to[s] as i32;
            bend = cn.max_bend(s) as f64;
            curve = bend_curve(&cn.bends[s]);
            techs = techs
                .with(Techs::SLIDE, slide_to >= 0)
                .with(Techs::UNPITCHED_SLIDE, slide_unpitch_to >= 0)
                .with(Techs::BEND, bend > 0.0)
                .with(Techs::VIBRATO, cn.vibrato[s] > 0);
        }
        notes.push(Note {
            time: n.time as f64,
            string: s as i32,
            fret,
            sustain: (n.sustain as f64).max(0.0),
            techniques: techs,
            slide_to,
            slide_unpitch_to,
            bend,
            bend_curve: curve,
        });
    }
    Ok(Chord {
        time: n.time as f64,
        chord_id: n.chord_id,
        name: template.name.clone(),
        notes,
        techniques: chord_techs,
        sustain: (n.sustain as f64).max(0.0),
        frets: template.frets.iter().map(|&f| f as i32).collect(),
        fingers: template.fingers.iter().map(|&f| f as i32).collect(),
    })
}

pub struct ArrMeta<'a> {
    pub name: &'a str,
    pub is_bass: bool,
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
}

/// The chart at a mastery (1.0 = full difficulty) as an Arrangement.
pub fn sng_to_arrangement(sng: &Sng, meta: &ArrMeta, mastery: f64) -> Result<Arrangement> {
    let m = &sng.metadata;
    let mut arr = Arrangement::new(meta.name);
    arr.title = meta.title.into();
    arr.artist = meta.artist.into();
    arr.album = meta.album.into();
    arr.tuning = if m.tuning.is_empty() {
        vec![0; 6]
    } else {
        m.tuning.iter().map(|&t| t as i32).collect()
    };
    arr.capo = m.capo;
    arr.song_length = m.song_length as f64;
    arr.is_bass = meta.is_bass;
    arr.source = "sng".into();
    arr.mastery = mastery;
    arr.has_levels = sng.has_levels();
    arr.beats = sng
        .beats
        .iter()
        .map(|b| Beat {
            time: b.time as f64,
            measure: if b.mask & BEAT_FIRST_OF_MEASURE != 0 {
                b.measure as i32
            } else {
                -1
            },
        })
        .collect();
    arr.sections = sng
        .sections
        .iter()
        .map(|s| Section {
            name: s.name.clone(),
            number: s.number,
            start: s.start as f64,
            end: s.end as f64,
        })
        .collect();
    arr.anchors = sng
        .anchors_at_mastery(mastery)
        .into_iter()
        .map(|a| Anchor {
            start: a.start as f64,
            end: a.end as f64,
            fret: a.fret as i32,
            width: a.width,
        })
        .collect();
    for h in sng.handshapes_at_mastery(mastery) {
        if let Some(t) = usize::try_from(h.chord_id)
            .ok()
            .and_then(|k| sng.chord_templates.get(k))
        {
            arr.handshapes.push(HandShape {
                start: h.start as f64,
                end: h.end as f64,
                name: t.name.clone(),
                frets: t.frets.iter().map(|&f| f as i32).collect(),
                fingers: t.fingers.iter().map(|&f| f as i32).collect(),
                arpeggio: h.arpeggio,
            });
        }
    }
    for n in sng.notes_at_mastery(mastery) {
        if n.is_chord() {
            arr.chords.push(chord_to_model(sng, n)?);
        } else {
            arr.notes.push(note_to_model(n));
        }
    }
    arr.notes
        .sort_by(|a, b| a.time.total_cmp(&b.time).then(a.string.cmp(&b.string)));
    arr.tones = sng
        .tones
        .iter()
        .map(|&(t, id)| ToneChange {
            time: t as f64,
            name: format!("Tone_{}", (b'A' + id.clamp(0, 3) as u8) as char),
        })
        .collect();
    Ok(arr)
}
