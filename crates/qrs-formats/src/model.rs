//! Format-independent note chart model. Port of rsplay/model.py.
//!
//! Every reader (SNG, XML, feedpak) produces an `Arrangement` holding the chart at one difficulty.
//! Strings are numbered as in Rocksmith: 0 is the lowest-pitched string (low E on guitar). Times are seconds.

use serde_json::{Map, Value, json};

/// A set of playing techniques (bit flags). JSON lists them by name, sorted.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Techs(pub u32);

macro_rules! techniques {
    ($($konst:ident = $bit:expr, $name:literal;)*) => {
        impl Techs {
            $(pub const $konst: Techs = Techs(1 << $bit);)*
            /// (flag, JSON name) for every technique.
            pub const ALL: &'static [(Techs, &'static str)] = &[$((Techs::$konst, $name)),*];
        }
    };
}

techniques! {
    HAMMER_ON = 0, "hammer_on";
    PULL_OFF = 1, "pull_off";
    SLIDE = 2, "slide";
    UNPITCHED_SLIDE = 3, "unpitched_slide";
    BEND = 4, "bend";
    VIBRATO = 5, "vibrato";
    PALM_MUTE = 6, "palm_mute";
    FRET_HAND_MUTE = 7, "fret_hand_mute";
    HARMONIC = 8, "harmonic";
    PINCH_HARMONIC = 9, "pinch_harmonic";
    TAP = 10, "tap";
    TREMOLO = 11, "tremolo";
    SLAP = 12, "slap";
    POP = 13, "pop";
    ACCENT = 14, "accent";
    LINK_NEXT = 15, "link_next";
    IGNORE = 16, "ignore";
    SUSTAIN = 17, "sustain";
}

impl Techs {
    pub const NONE: Techs = Techs(0);

    pub fn has(self, t: Techs) -> bool {
        self.0 & t.0 != 0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn insert(&mut self, t: Techs) {
        self.0 |= t.0;
    }
    pub fn remove(&mut self, t: Techs) {
        self.0 &= !t.0;
    }
    pub fn with(mut self, t: Techs, on: bool) -> Techs {
        if on {
            self.insert(t)
        }
        self
    }
    pub fn from_name(name: &str) -> Option<Techs> {
        Techs::ALL.iter().find(|(_, n)| *n == name).map(|(t, _)| *t)
    }
    /// Names, sorted alphabetically (as Python's `sorted(techniques)`).
    pub fn names(self) -> Vec<&'static str> {
        let mut v: Vec<&str> = Techs::ALL
            .iter()
            .filter(|(t, _)| self.has(*t))
            .map(|(_, n)| *n)
            .collect();
        v.sort_unstable();
        v
    }
}

impl std::ops::BitOr for Techs {
    type Output = Techs;
    fn bitor(self, o: Techs) -> Techs {
        Techs(self.0 | o.0)
    }
}
impl std::ops::BitOrAssign for Techs {
    fn bitor_assign(&mut self, o: Techs) {
        self.0 |= o.0
    }
}
impl std::ops::BitAnd for Techs {
    type Output = Techs;
    fn bitand(self, o: Techs) -> Techs {
        Techs(self.0 & o.0)
    }
}
impl std::ops::Sub for Techs {
    type Output = Techs;
    fn sub(self, o: Techs) -> Techs {
        Techs(self.0 & !o.0)
    }
}
impl std::fmt::Debug for Techs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.names()).finish()
    }
}

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
/// E2 A2 D3 G3 B3 E4
pub const GUITAR_OPEN_MIDI: [i32; 6] = [40, 45, 50, 55, 59, 64];
/// E1 A1 D2 G2 (+ B, E for the unused upper strings)
pub const BASS_OPEN_MIDI: [i32; 6] = [28, 33, 38, 43, 47, 52];

pub fn note_name(midi: i32) -> &'static str {
    NOTE_NAMES[midi.rem_euclid(12) as usize]
}

/// Python's `round(x, 3)` as a JSON number.
pub fn r3(x: f64) -> Value {
    let v: f64 = format!("{x:.3}").parse().unwrap_or(x);
    json!(v)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub time: f64,
    pub string: i32,
    pub fret: i32,
    pub sustain: f64,
    pub techniques: Techs,
    pub slide_to: i32,
    pub slide_unpitch_to: i32,
    /// Maximum bend, in half steps (Rocksmith's and feedpak's unit).
    pub bend: f64,
    /// Bend curve (time, step) when the source has one.
    pub bend_curve: Vec<(f64, f64)>,
}

impl Note {
    pub fn new(time: f64, string: i32, fret: i32) -> Note {
        Note {
            time,
            string,
            fret,
            sustain: 0.0,
            techniques: Techs::NONE,
            slide_to: -1,
            slide_unpitch_to: -1,
            bend: 0.0,
            bend_curve: Vec::new(),
        }
    }

    pub fn has(&self, t: Techs) -> bool {
        self.techniques.has(t)
    }

    pub fn to_json(&self) -> Value {
        let mut d = Map::new();
        d.insert("time".into(), r3(self.time));
        d.insert("string".into(), json!(self.string));
        d.insert("fret".into(), json!(self.fret));
        if self.sustain > 0.0 {
            d.insert("sustain".into(), r3(self.sustain));
        }
        if !self.techniques.is_empty() {
            d.insert("techniques".into(), json!(self.techniques.names()));
        }
        if self.slide_to >= 0 {
            d.insert("slide_to".into(), json!(self.slide_to));
        }
        if self.slide_unpitch_to >= 0 {
            d.insert("slide_unpitch_to".into(), json!(self.slide_unpitch_to));
        }
        if self.bend > 0.0 {
            d.insert("bend".into(), r3(self.bend));
        }
        Value::Object(d)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    pub time: f64,
    pub chord_id: i32,
    pub name: String,
    pub notes: Vec<Note>,
    pub techniques: Techs,
    pub sustain: f64,
    /// The chord shape, low string first; -1 = not played.
    pub frets: Vec<i32>,
    /// 1-4 = index..pinky, 0 = thumb, -1 = none.
    pub fingers: Vec<i32>,
}

impl Chord {
    pub fn to_json(&self) -> Value {
        let mut d = Map::new();
        d.insert("time".into(), r3(self.time));
        d.insert("chord_id".into(), json!(self.chord_id));
        d.insert("name".into(), json!(self.name));
        d.insert(
            "notes".into(),
            Value::Array(self.notes.iter().map(Note::to_json).collect()),
        );
        if !self.techniques.is_empty() {
            d.insert("techniques".into(), json!(self.techniques.names()));
        }
        if self.sustain > 0.0 {
            d.insert("sustain".into(), r3(self.sustain));
        }
        Value::Object(d)
    }
}

/// The fret window the fretting hand sits in from start to end.
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub start: f64,
    pub end: f64,
    pub fret: i32,
    pub width: i32,
}

/// A chord shape the fretting hand holds from start to end, e.g. while picking through an arpeggio.
#[derive(Clone, Debug, PartialEq)]
pub struct HandShape {
    pub start: f64,
    pub end: f64,
    pub name: String,
    pub frets: Vec<i32>,
    pub fingers: Vec<i32>,
    pub arpeggio: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Beat {
    pub time: f64,
    /// Measure number on a downbeat, -1 on other beats.
    pub measure: i32,
}

impl Beat {
    pub fn is_downbeat(&self) -> bool {
        self.measure >= 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    pub number: i32,
    pub start: f64,
    pub end: f64,
}

/// A tone change in the chart (Rocksmith's Tone_A..D, feedpak `tones.changes`).
#[derive(Clone, Debug, PartialEq)]
pub struct ToneChange {
    pub time: f64,
    pub name: String,
}

/// A note or chord, in time order.
#[derive(Clone, Copy, Debug)]
pub enum Event<'a> {
    Note(&'a Note),
    Chord(&'a Chord),
}

impl Event<'_> {
    pub fn time(&self) -> f64 {
        match self {
            Event::Note(n) => n.time,
            Event::Chord(c) => c.time,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Arrangement {
    /// e.g. "Lead", "Rhythm", "Bass", "Combo".
    pub name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Semitone offsets per string.
    pub tuning: Vec<i32>,
    pub capo: i32,
    /// Cents off concert pitch (feedpak `centOffset`, Rocksmith's A440 offset).
    pub cent_offset: f64,
    pub song_length: f64,
    pub is_bass: bool,
    pub beats: Vec<Beat>,
    pub sections: Vec<Section>,
    pub notes: Vec<Note>,
    pub chords: Vec<Chord>,
    /// "sng", "xml" or "feedpak".
    pub source: String,
    /// Difficulty the notes were taken at (1.0 = full chart).
    pub mastery: f64,
    /// Whether the chart has a difficulty ladder (the Easier / Harder control is gated on this).
    pub has_levels: bool,
    pub handshapes: Vec<HandShape>,
    pub anchors: Vec<Anchor>,
    pub tones: Vec<ToneChange>,
}

impl Arrangement {
    pub fn new(name: &str) -> Arrangement {
        Arrangement {
            name: name.into(),
            tuning: vec![0; 6],
            mastery: 1.0,
            ..Default::default()
        }
    }

    pub fn string_count(&self) -> usize {
        let base = if self.is_bass { 4 } else { 6 };
        let used = self
            .notes
            .iter()
            .map(|n| n.string)
            .chain(self.chords.iter().flat_map(|c| c.notes.iter().map(|n| n.string)))
            .map(|s| (s + 1).max(0) as usize)
            .max()
            .unwrap_or(0);
        base.max(used)
    }

    /// Notes and chords merged in time order (stable: notes before chords at equal times, as Python's sorted()).
    pub fn events(&self) -> Vec<Event<'_>> {
        let mut v: Vec<Event> = self
            .notes
            .iter()
            .map(Event::Note)
            .chain(self.chords.iter().map(Event::Chord))
            .collect();
        v.sort_by(|a, b| a.time().total_cmp(&b.time()));
        v
    }

    pub fn open_string_midi(&self) -> Vec<i32> {
        let base = if self.is_bass {
            BASS_OPEN_MIDI
        } else {
            GUITAR_OPEN_MIDI
        };
        let mut offsets = self.tuning.clone();
        offsets.resize(offsets.len().max(6), 0);
        (0..self.string_count())
            .map(|i| base[i.min(5)] + offsets.get(i).copied().unwrap_or(0))
            .collect()
    }

    pub fn string_names(&self) -> Vec<&'static str> {
        self.open_string_midi().into_iter().map(note_name).collect()
    }

    pub fn tuning_name(&self) -> String {
        tuning_name(&self.tuning, self.is_bass)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "title": self.title,
            "artist": self.artist,
            "album": self.album,
            "arrangement": self.name,
            "source": self.source,
            "is_bass": self.is_bass,
            "tuning": {"offsets": self.tuning, "name": self.tuning_name(), "strings": self.string_names()},
            "capo": self.capo,
            "song_length": r3(self.song_length),
            "beats": self.beats.iter().map(|b| json!({"time": r3(b.time), "measure": b.measure})).collect::<Vec<_>>(),
            "sections": self.sections.iter().map(|s| json!({
                "name": s.name, "number": s.number, "start": r3(s.start), "end": r3(s.end)
            })).collect::<Vec<_>>(),
            "notes": self.notes.iter().map(Note::to_json).collect::<Vec<_>>(),
            "chords": self.chords.iter().map(Chord::to_json).collect::<Vec<_>>(),
        })
    }
}

/// "E Standard", "Drop D", "Eb Standard" or "Custom".
pub fn tuning_name(tuning: &[i32], is_bass: bool) -> String {
    let n = if is_bass { 4 } else { 6 };
    let mut offs: Vec<i32> = tuning.to_vec();
    offs.resize(offs.len().max(6), 0);
    offs.truncate(n);
    let low = if is_bass {
        BASS_OPEN_MIDI[0]
    } else {
        GUITAR_OPEN_MIDI[0]
    };
    if offs.iter().all(|&o| o == offs[0]) {
        return format!("{} Standard", note_name(low + offs[0]));
    }
    if offs[1..].iter().all(|&o| o == offs[1]) && offs[0] == offs[1] - 2 {
        return format!("Drop {}", note_name(low + offs[0]));
    }
    "Custom".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tuning_names() {
        assert_eq!(tuning_name(&[0; 6], false), "E Standard");
        assert_eq!(tuning_name(&[-1; 6], false), "Eb Standard");
        assert_eq!(tuning_name(&[-2, 0, 0, 0, 0, 0], false), "Drop D");
        assert_eq!(tuning_name(&[-4, -2, -2, -2, -2, -2], false), "Drop C");
        assert_eq!(tuning_name(&[0, 0, 0, 0], true), "E Standard");
        assert_eq!(tuning_name(&[-2, 0, 0, 1, 0, 0], false), "Custom");
    }

    #[test]
    fn rounding_matches_python() {
        assert_eq!(r3(1.0).to_string(), "1.0");
        assert_eq!(r3(1.23456).to_string(), "1.235");
        assert_eq!(r3(26.825000762939453).to_string(), "26.825");
        assert_eq!(r3(-0.0001).to_string(), "-0.0"); // as Python
    }

    #[test]
    fn technique_names_sorted() {
        let t = Techs::VIBRATO | Techs::BEND | Techs::SUSTAIN;
        assert_eq!(t.names(), vec!["bend", "sustain", "vibrato"]);
    }
}
