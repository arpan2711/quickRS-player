//! Builders for synthetic Rocksmith files, so tests need no real songs or real keys. Port of tests/builders.py.
//!
//! Formats are written out explicitly here (not through the parser's own types) so that a layout mistake in the
//! parser is not silently mirrored by the builder.

use crate::keys::Key;
use crate::psarc::write_psarc;
use crate::sng;

/// Dummy keys for tests only: bytes(range(32)) and bytes(range(100, 132)).
pub const TEST_PSARC_KEY: Key = seq(0);
pub const TEST_SNG_KEY: Key = seq(100);
pub const TEST_IV: [u8; 16] = {
    let mut a = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        a[i] = i as u8;
        i += 1;
    }
    a
};

const fn seq(start: u8) -> Key {
    let mut k = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        k[i] = start + i as u8;
        i += 1;
    }
    k
}

// SNG mask bits (duplicated deliberately, see module docs)
pub const CHORD: u32 = 0x2;
pub const FRET_HAND_MUTE_CHORD: u32 = 0x8;
pub const HARMONIC: u32 = 0x20;
pub const PALM_MUTE: u32 = 0x40;
pub const HAMMER_ON: u32 = 0x200;
pub const PULL_OFF: u32 = 0x400;
pub const SLIDE: u32 = 0x800;
pub const BEND: u32 = 0x1000;
pub const SUSTAIN: u32 = 0x2000;
pub const VIBRATO: u32 = 0x10000;
pub const MUTE: u32 = 0x20000;
pub const CHORD_NOTES: u32 = 0x1000000;
pub const CHORD_PANEL: u32 = 0x80000000;

#[derive(Clone, Debug)]
pub struct TNote {
    pub time: f32,
    pub string: i8,
    pub fret: i8,
    pub mask: u32,
    pub chord_id: i32,
    pub chord_notes_id: i32,
    pub slide_to: i8,
    pub slide_unpitch_to: i8,
    pub tap: i8,
    pub vibrato: i16,
    pub sustain: f32,
    pub bends: Vec<(f32, f32)>,
}

pub fn note(time: f32, string: i8, fret: i8, mask: u32) -> TNote {
    TNote {
        time,
        string,
        fret,
        mask,
        chord_id: -1,
        chord_notes_id: -1,
        slide_to: -1,
        slide_unpitch_to: -1,
        tap: -1,
        vibrato: 0,
        sustain: 0.0,
        bends: Vec::new(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct TChordNotes {
    pub masks: [u32; 6],
    pub bends: [Vec<(f32, f32)>; 6],
    pub slide_to: Option<[i8; 6]>,
    pub slide_unpitch_to: Option<[i8; 6]>,
    pub vibrato: [i16; 6],
}

#[derive(Clone, Debug)]
pub struct SngSpec {
    /// (time, measure, beat, mask)
    pub beats: Vec<(f32, i16, i16, i32)>,
    /// (max difficulty, name)
    pub phrases: Vec<(i32, String)>,
    /// (frets, name)
    pub chord_templates: Vec<([i8; 6], String)>,
    pub chord_notes: Vec<TChordNotes>,
    /// (phrase id, start, end)
    pub phrase_iterations: Vec<(i32, f32, f32)>,
    /// (name, number, start, end)
    pub sections: Vec<(String, i32, f32, f32)>,
    /// (difficulty, notes)
    pub levels: Vec<(i32, Vec<TNote>)>,
    pub tuning: Vec<i16>,
    pub capo: i8,
    pub song_length: f32,
    pub vocals: usize,
}

struct W(Vec<u8>);
impl W {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn i8(&mut self, v: i8) {
        self.0.push(v as u8)
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn zeros(&mut self, n: usize) {
        self.0.extend(std::iter::repeat_n(0u8, n))
    }
    fn cstr(&mut self, s: &str, n: usize) {
        let b = s.as_bytes();
        let k = b.len().min(n - 1);
        self.0.extend_from_slice(&b[..k]);
        self.zeros(n - k);
    }
}

pub fn build_sng_plain(s: &SngSpec) -> Vec<u8> {
    let mut o = W(Vec::new());
    o.u32(s.beats.len() as u32);
    for &(t, measure, beat, mask) in &s.beats {
        o.f32(t);
        o.i16(measure);
        o.i16(beat);
        o.i32(0);
        o.i32(mask);
    }
    o.u32(s.phrases.len() as u32);
    for (max_diff, name) in &s.phrases {
        o.zeros(4);
        o.i32(*max_diff);
        o.i32(0);
        o.cstr(name, 32);
    }
    o.u32(s.chord_templates.len() as u32);
    for (frets, name) in &s.chord_templates {
        o.u32(0);
        for f in frets {
            o.i8(*f);
        }
        for _ in 0..6 {
            o.i8(-1);
        }
        for _ in 0..6 {
            o.i32(-1);
        }
        o.cstr(name, 32);
    }
    o.u32(s.chord_notes.len() as u32);
    for cn in &s.chord_notes {
        for m in cn.masks {
            o.u32(m);
        }
        for st in 0..6 {
            let values = &cn.bends[st];
            for j in 0..32 {
                let (t, step) = values.get(j).copied().unwrap_or((0.0, 0.0));
                o.f32(t);
                o.f32(step);
                o.i32(0);
            }
            o.i32(values.len() as i32);
        }
        for v in cn.slide_to.unwrap_or([-1; 6]) {
            o.i8(v);
        }
        for v in cn.slide_unpitch_to.unwrap_or([-1; 6]) {
            o.i8(v);
        }
        for v in cn.vibrato {
            o.i16(v);
        }
    }
    o.u32(s.vocals as u32);
    for i in 0..s.vocals {
        o.f32(i as f32);
        o.i32(60);
        o.f32(0.5);
        o.cstr("la", 48);
    }
    if s.vocals > 0 {
        o.u32(1);
        o.zeros(32); // symbols header
        o.u32(1);
        o.cstr("font", 128);
        for v in [4, 0, 256, 256] {
            o.i32(v);
        } // texture
        o.u32(1);
        o.cstr("la", 12);
        o.zeros(32); // definition
    }
    o.u32(s.phrase_iterations.len() as u32);
    for &(phrase_id, start, end) in &s.phrase_iterations {
        o.i32(phrase_id);
        o.f32(start);
        o.f32(end);
        o.zeros(12);
    }
    o.u32(0); // phrase extra info
    o.u32(1); // one linked difficulty
    o.i32(-1);
    o.u32(2);
    o.i32(0);
    o.i32(1);
    o.u32(0); // actions
    o.u32(1); // one event
    o.f32(1.0);
    o.cstr("e0", 256);
    o.u32(1); // tones
    o.f32(0.0);
    o.i32(0);
    o.u32(1); // DNAs
    o.f32(0.0);
    o.i32(1);
    o.u32(s.sections.len() as u32);
    for (name, number, start, end) in &s.sections {
        o.cstr(name, 32);
        o.i32(*number);
        o.f32(*start);
        o.f32(*end);
        o.i32(0);
        o.i32(0);
        o.zeros(36);
    }
    o.u32(s.levels.len() as u32);
    for (difficulty, notes) in &s.levels {
        o.i32(*difficulty);
        o.u32(1);
        o.zeros(28); // one anchor
        o.u32(0); // anchor extensions
        o.u32(1);
        o.zeros(20); // one hand shape
        o.u32(0); // arpeggios
        o.u32(notes.len() as u32);
        for n in notes {
            let max_bend = n.bends.iter().map(|b| b.1).fold(0.0f32, f32::max);
            o.u32(n.mask);
            o.u32(0);
            o.u32(0);
            o.f32(n.time);
            o.i8(n.string);
            o.i8(n.fret);
            o.i8(0);
            o.i8(4);
            o.i32(n.chord_id);
            o.i32(n.chord_notes_id);
            o.i32(0);
            o.i32(0);
            for _ in 0..5 {
                o.i16(-1);
            }
            o.i8(n.slide_to);
            o.i8(n.slide_unpitch_to);
            o.i8(-1);
            o.i8(n.tap);
            o.i8(0);
            o.i8(-1);
            o.i8(-1);
            o.i16(n.vibrato);
            o.f32(n.sustain);
            o.f32(max_bend);
            o.u32(n.bends.len() as u32);
            for &(t, step) in &n.bends {
                o.f32(t);
                o.f32(step);
                o.i32(0);
            }
        }
        o.u32(1);
        o.f32(1.0);
        o.u32(1);
        o.i32(notes.len() as i32);
        o.u32(1);
        o.i32(notes.len() as i32);
    }
    for v in [0.0, 0.0, 0.0, 0.0] {
        o.f64(v);
    }
    o.f32(0.5);
    o.f32(0.0);
    o.i8(s.capo);
    o.cstr("1-1-26 12:00", 32);
    o.i16(1);
    o.f32(s.song_length);
    o.u32(s.tuning.len() as u32);
    for t in &s.tuning {
        o.i16(*t);
    }
    o.f32(1.0);
    o.f32(1.0);
    o.i32(s.levels.iter().map(|l| l.0).max().unwrap_or(0));
    o.0
}

pub fn pack_test_sng(plain: &[u8]) -> Vec<u8> {
    sng::pack_sng(plain, &TEST_SNG_KEY, &TEST_IV)
}

// --- a small demo chart, available as SNG and as XML ----------------------------------------------------
// 120 bpm, 4/4, beats every 0.5 s. Phrases: COUNT (0-1 s), riff (1-5 s, max difficulty 1), END (5-6.5 s).
// Level 0 holds a simplified riff that must NOT appear in the full-difficulty chart.
pub fn demo_spec() -> SngSpec {
    let beats = (0..14)
        .map(|i| {
            (
                i as f32 * 0.5,
                (i / 4 + 1) as i16,
                (i % 4) as i16,
                (i % 4 == 0) as i32,
            )
        })
        .collect();
    let mut cn = TChordNotes::default();
    cn.masks[2] = VIBRATO;
    cn.vibrato[2] = 80;
    let level1 = vec![
        note(1.0, 0, 3, 0),
        note(1.25, 0, 5, HAMMER_ON),
        TNote {
            chord_id: 0,
            chord_notes_id: 0,
            ..note(1.5, -1, -1, CHORD | PALM_MUTE | CHORD_NOTES | CHORD_PANEL)
        },
        TNote {
            sustain: 0.4,
            bends: vec![(2.1, 0.5), (2.2, 1.0)],
            ..note(2.0, 3, 7, BEND | SUSTAIN)
        },
        TNote {
            slide_to: 7,
            ..note(2.5, 4, 5, SLIDE)
        },
        note(3.0, 1, 0, MUTE),
        note(3.5, 0, 5, PULL_OFF),
        TNote {
            chord_id: 1,
            ..note(4.0, -1, -1, CHORD | FRET_HAND_MUTE_CHORD)
        },
        note(5.5, 3, 99, 0), // in the END phrase (max difficulty 0): must be ignored
    ];
    SngSpec {
        beats,
        phrases: vec![(0, "COUNT".into()), (1, "riff".into()), (0, "END".into())],
        chord_templates: vec![
            ([0, 2, 2, -1, -1, -1], "E5".into()),
            ([3, 2, 0, 0, 0, 3], "G".into()),
        ],
        chord_notes: vec![cn],
        phrase_iterations: vec![(0, 0.0, 1.0), (1, 1.0, 5.0), (2, 5.0, 6.5)],
        sections: vec![("riff".into(), 1, 1.0, 5.0), ("noguitar".into(), 1, 5.0, 6.5)],
        levels: vec![
            (0, vec![note(1.0, 0, 3, 0), note(5.5, 3, 12, HARMONIC)]),
            (1, level1),
        ],
        tuning: vec![-2, 0, 0, 0, 0, 0],
        capo: -1,
        song_length: 6.5,
        vocals: 0,
    }
}

pub fn demo_sng_plain() -> Vec<u8> {
    build_sng_plain(&demo_spec())
}

pub fn demo_xml() -> String {
    let beats: String = (0..14)
        .map(|i| {
            let m = if i % 4 == 0 { i / 4 + 1 } else { -1 };
            format!("    <ebeat time=\"{:.3}\" measure=\"{m}\" />\n", i as f64 * 0.5)
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<song version="8">
  <title>Demo Song</title>
  <arrangement>Lead</arrangement>
  <songLength>6.500</songLength>
  <tuning string0="-2" string1="0" string2="0" string3="0" string4="0" string5="0" />
  <capo>0</capo>
  <artistName>Test Band</artistName>
  <albumName>Fixtures</albumName>
  <phrases count="3">
    <phrase maxDifficulty="0" name="COUNT" />
    <phrase maxDifficulty="1" name="riff" />
    <phrase maxDifficulty="0" name="END" />
  </phrases>
  <phraseIterations count="3">
    <phraseIteration time="0.000" phraseId="0" />
    <phraseIteration time="1.000" phraseId="1" />
    <phraseIteration time="5.000" phraseId="2" />
  </phraseIterations>
  <chordTemplates count="2">
    <chordTemplate chordName="E5" displayName="E5" fret0="0" fret1="2" fret2="2" fret3="-1" fret4="-1" fret5="-1" />
    <chordTemplate chordName="G" displayName="G" fret0="3" fret1="2" fret2="0" fret3="0" fret4="0" fret5="3" />
  </chordTemplates>
  <ebeats count="14">
{beats}  </ebeats>
  <sections count="2">
    <section name="riff" number="1" startTime="1.000" />
    <section name="noguitar" number="1" startTime="5.000" />
  </sections>
  <levels count="2">
    <level difficulty="0">
      <notes count="2">
        <note time="1.000" string="0" fret="3" />
        <note time="5.500" string="3" fret="12" harmonic="1" />
      </notes>
      <chords count="0" />
    </level>
    <level difficulty="1">
      <notes count="7">
        <note time="1.000" string="0" fret="3" />
        <note time="1.250" string="0" fret="5" hammerOn="1" />
        <note time="2.000" string="3" fret="7" sustain="0.400" bend="1">
          <bendValues count="2"><bendValue time="2.100" step="0.5" /><bendValue time="2.200" step="1" /></bendValues>
        </note>
        <note time="2.500" string="4" fret="5" slideTo="7" />
        <note time="3.000" string="1" fret="0" mute="1" />
        <note time="3.500" string="0" fret="5" pullOff="1" />
        <note time="5.500" string="3" fret="99" />
      </notes>
      <chords count="2">
        <chord time="1.500" chordId="0" palmMute="1">
          <chordNote time="1.500" string="0" fret="0" palmMute="1" />
          <chordNote time="1.500" string="1" fret="2" palmMute="1" />
          <chordNote time="1.500" string="2" fret="2" palmMute="1" vibrato="80" />
        </chord>
        <chord time="4.000" chordId="1" fretHandMute="1" />
      </chords>
    </level>
  </levels>
</song>
"#
    )
}

pub fn demo_manifest(name: &str, dlc: &str, path_bass: i32) -> Vec<u8> {
    let lower = name.to_lowercase();
    let tuning: serde_json::Map<String, serde_json::Value> = (0..6)
        .map(|i| {
            (
                format!("string{i}"),
                serde_json::json!(if i == 0 { -2 } else { 0 }),
            )
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({
        "Entries": {"ABC123": {"Attributes": {
            "ArrangementName": name, "SongName": "Demo Song", "ArtistName": "Test Band", "AlbumName": "Fixtures",
            "SongAsset": format!("urn:application:musicgame-song:{dlc}_{lower}"),
            "SongXml": format!("urn:application:xml:{dlc}_{lower}"),
            "Tuning": tuning, "CapoFret": 0, "SongLength": 6.5,
            "ArrangementProperties": {"pathBass": path_bass, "bonusArr": 0}
        }}},
        "ModelName": "RSEnumerable_Song"
    }))
    .unwrap()
}

pub fn demo_psarc_files() -> Vec<(String, Vec<u8>)> {
    let sng = pack_test_sng(&demo_sng_plain());
    let vocals = serde_json::json!({"Entries": {"x": {"Attributes": {
        "ArrangementName": "Vocals", "SongAsset": "urn:application:musicgame-song:demo_vocals"}}}});
    let mut wem_full = b"RIFF".to_vec();
    wem_full.extend(std::iter::repeat_n(0u8, 5000));
    let mut wem_preview = b"RIFF".to_vec();
    wem_preview.extend(std::iter::repeat_n(0u8, 500));
    vec![
        ("songs/bin/generic/demo_lead.sng".into(), sng.clone()),
        ("songs/bin/generic/demo_bass.sng".into(), sng),
        (
            "manifests/songs_dlc_demo/demo_lead.json".into(),
            demo_manifest("Lead", "demo", 0),
        ),
        (
            "manifests/songs_dlc_demo/demo_bass.json".into(),
            demo_manifest("Bass", "demo", 1),
        ),
        (
            "manifests/songs_dlc_demo/demo_vocals.json".into(),
            serde_json::to_vec(&vocals).unwrap(),
        ),
        ("audio/windows/111.wem".into(), wem_full),
        ("audio/windows/222.wem".into(), wem_preview),
    ]
}

pub fn demo_psarc(key: Option<&Key>) -> Vec<u8> {
    write_psarc(&demo_psarc_files(), key, 65536, true)
}

pub fn test_keys() -> crate::keys::Keys {
    crate::keys::Keys {
        psarc: Some(TEST_PSARC_KEY),
        sng: Some(TEST_SNG_KEY),
        sng_mac: None,
        source: None,
    }
}
