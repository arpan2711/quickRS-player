//! Keys, scales, chords and the fretboard of notes that fit (the jam's backing and its scale map). Pure functions,
//! no audio. New for quickRS (P22.4); the Python app has no jam room.
//!
//! Pitch classes are 0..12 from C. Chord progressions are written as names ("Am F C G") or as Roman numerals in
//! the key ("I-V-vi-IV", "i VI III VII", "bVII"): upper case is major, lower case minor, `°`/`o` diminished,
//! `+` augmented, a trailing 7 the chord's seventh.

/// Note names as the jam page shows them (sharps for C#/F#, flats elsewhere).
pub use qrs_formats::model::NOTE_NAMES;

pub fn note_name(pc: u8) -> &'static str {
    NOTE_NAMES[(pc % 12) as usize]
}

/// A note name ("A", "Bb", "F#", "Db") as a pitch class.
pub fn parse_note(s: &str) -> Option<u8> {
    let mut ch = s.chars();
    let base = match ch.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut pc: i32 = base;
    for c in ch {
        match c {
            '#' | '♯' => pc += 1,
            'b' | '♭' => pc -= 1,
            _ => return None,
        }
    }
    Some(pc.rem_euclid(12) as u8)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scale {
    Major,
    NaturalMinor,
    MinorPentatonic,
    MajorPentatonic,
    Blues,
    Dorian,
    Mixolydian,
    Phrygian,
    Lydian,
    HarmonicMinor,
}

impl Scale {
    pub const ALL: [Scale; 10] = [
        Scale::MinorPentatonic,
        Scale::MajorPentatonic,
        Scale::Blues,
        Scale::Major,
        Scale::NaturalMinor,
        Scale::Dorian,
        Scale::Mixolydian,
        Scale::Phrygian,
        Scale::Lydian,
        Scale::HarmonicMinor,
    ];

    /// Semitones above the root.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::MinorPentatonic => &[0, 3, 5, 7, 10],
            Scale::MajorPentatonic => &[0, 2, 4, 7, 9],
            Scale::Blues => &[0, 3, 5, 6, 7, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Scale::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scale::Major => "Major",
            Scale::NaturalMinor => "Natural minor",
            Scale::MinorPentatonic => "Minor pentatonic",
            Scale::MajorPentatonic => "Major pentatonic",
            Scale::Blues => "Blues",
            Scale::Dorian => "Dorian",
            Scale::Mixolydian => "Mixolydian",
            Scale::Phrygian => "Phrygian",
            Scale::Lydian => "Lydian",
            Scale::HarmonicMinor => "Harmonic minor",
        }
    }

    /// Whether the scale's third is minor (Roman numerals and drones read the key as minor).
    pub fn is_minor(self) -> bool {
        self.intervals().contains(&3) && !self.intervals().contains(&4)
    }

    /// The seven-note scale chords are built from (pentatonics and the blues borrow their parent's).
    pub fn heptatonic(self) -> Scale {
        match self {
            Scale::MinorPentatonic | Scale::Blues => Scale::NaturalMinor,
            Scale::MajorPentatonic => Scale::Major,
            s => s,
        }
    }
}

/// The scale's pitch classes from the root up.
pub fn scale_notes(root: u8, scale: Scale) -> Vec<u8> {
    scale.intervals().iter().map(|i| (root + i) % 12).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Quality {
    Major,
    Minor,
    Dim,
    Aug,
    Sus2,
    Sus4,
    Power,
    Dom7,
    Maj7,
    Min7,
}

impl Quality {
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Quality::Major => &[0, 4, 7],
            Quality::Minor => &[0, 3, 7],
            Quality::Dim => &[0, 3, 6],
            Quality::Aug => &[0, 4, 8],
            Quality::Sus2 => &[0, 2, 7],
            Quality::Sus4 => &[0, 5, 7],
            Quality::Power => &[0, 7],
            Quality::Dom7 => &[0, 4, 7, 10],
            Quality::Maj7 => &[0, 4, 7, 11],
            Quality::Min7 => &[0, 3, 7, 10],
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Quality::Major => "",
            Quality::Minor => "m",
            Quality::Dim => "dim",
            Quality::Aug => "aug",
            Quality::Sus2 => "sus2",
            Quality::Sus4 => "sus4",
            Quality::Power => "5",
            Quality::Dom7 => "7",
            Quality::Maj7 => "maj7",
            Quality::Min7 => "m7",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub root: u8,
    pub quality: Quality,
}

impl Chord {
    pub fn new(root: u8, quality: Quality) -> Chord {
        Chord {
            root: root % 12,
            quality,
        }
    }

    /// "Am", "F", "C#m7".
    pub fn name(&self) -> String {
        format!("{}{}", note_name(self.root), self.quality.suffix())
    }

    /// The chord's pitch classes, root first.
    pub fn tones(&self) -> Vec<u8> {
        self.quality
            .intervals()
            .iter()
            .map(|i| (self.root + i) % 12)
            .collect()
    }

    /// The chord's notes as MIDI numbers, a close voicing with its lowest note at or above `low`.
    pub fn voicing(&self, low: u8) -> Vec<u8> {
        let mut base = low - low % 12 + self.root;
        if base < low {
            base += 12;
        }
        // keep the voicing low enough not to sit above the guitar's range
        if base >= low + 7 {
            base -= 12;
        }
        let base = base.max(low.saturating_sub(5));
        self.quality.intervals().iter().map(|i| base + i).collect()
    }
}

/// A chord name: a note, then m / min / dim / ° / aug / + / sus2 / sus4 / 5 / 7 / maj7 / m7.
pub fn parse_chord(s: &str) -> Option<Chord> {
    let s = s.trim();
    let split = s
        .char_indices()
        .skip(1)
        .find(|(_, c)| !matches!(c, '#' | 'b' | '♯' | '♭'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    // "Bb" is B flat, but "Bbm" too; a lone "b" after the letter is always a flat
    let root = parse_note(&s[..split])?;
    let q = match &s[split..] {
        "" | "M" | "maj" => Quality::Major,
        "m" | "min" | "-" => Quality::Minor,
        "dim" | "°" | "o" => Quality::Dim,
        "aug" | "+" => Quality::Aug,
        "sus2" => Quality::Sus2,
        "sus4" | "sus" => Quality::Sus4,
        "5" => Quality::Power,
        "7" => Quality::Dom7,
        "maj7" | "M7" => Quality::Maj7,
        "m7" | "min7" | "-7" => Quality::Min7,
        _ => return None,
    };
    Some(Chord::new(root, q))
}

const ROMAN: [&str; 7] = ["i", "ii", "iii", "iv", "v", "vi", "vii"];

/// The diatonic triad on scale degree `degree` (0-based) of the key (pentatonics use their parent scale).
pub fn diatonic_triad(root: u8, scale: Scale, degree: usize) -> Chord {
    let iv = scale.heptatonic().intervals();
    let at = |k: usize| iv[k % 7] as i32 + 12 * (k / 7) as i32;
    let r = at(degree);
    let third = at(degree + 2) - r;
    let fifth = at(degree + 4) - r;
    let quality = match (third, fifth) {
        (4, 7) => Quality::Major,
        (3, 7) => Quality::Minor,
        (3, 6) => Quality::Dim,
        (4, 8) => Quality::Aug,
        _ => Quality::Major,
    };
    Chord::new(((root as i32 + r) % 12) as u8, quality)
}

/// A Roman numeral in the key: "vi", "IV", "bVII", "V7", "ii°", "#iv".
pub fn parse_roman(s: &str, root: u8, scale: Scale) -> Option<Chord> {
    let s = s.trim();
    let mut shift = 0i32;
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        match c {
            'b' | '♭' => shift -= 1,
            '#' | '♯' => shift += 1,
            _ => break,
        }
        rest = &rest[c.len_utf8()..];
    }
    let numeral_len = rest
        .char_indices()
        .find(|(_, c)| !matches!(c.to_ascii_lowercase(), 'i' | 'v'))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    let (numeral, suffix) = rest.split_at(numeral_len);
    let degree = ROMAN.iter().position(|r| r.eq_ignore_ascii_case(numeral))?;
    let upper = numeral.chars().all(|c| c.is_ascii_uppercase());
    let lower = numeral.chars().all(|c| c.is_ascii_lowercase());
    if !upper && !lower {
        return None;
    }
    // the degree's root as the major scale (or the key's own scale, for unaltered numerals) has it
    let iv = scale.heptatonic().intervals();
    let pc = (root as i32 + iv[degree] as i32 + shift).rem_euclid(12) as u8;
    let quality = match (suffix, upper) {
        ("", true) => Quality::Major,
        ("", false) => Quality::Minor,
        ("°" | "o" | "dim", _) => Quality::Dim,
        ("+" | "aug", _) => Quality::Aug,
        ("7", true) => Quality::Dom7,
        ("7", false) => Quality::Min7,
        ("maj7" | "M7", _) => Quality::Maj7,
        ("5", _) => Quality::Power,
        ("sus4" | "sus", _) => Quality::Sus4,
        ("sus2", _) => Quality::Sus2,
        _ => return None,
    };
    Some(Chord::new(pc, quality))
}

/// A progression of chord names or Roman numerals, separated by spaces, dashes, commas or bars.
pub fn parse_progression(s: &str, root: u8, scale: Scale) -> Result<Vec<Chord>, String> {
    let tokens: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | ',' | '|'))
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return Err("no chords".into());
    }
    tokens
        .iter()
        .map(|t| {
            parse_chord(t)
                .or_else(|| parse_roman(t, root, scale))
                .ok_or_else(|| format!("not a chord: {t}"))
        })
        .collect()
}

/// A chord as a Roman numeral in the key ("VI", "iv", "bVII"); chord qualities beyond major/minor are suffixed.
pub fn roman_of(chord: &Chord, root: u8, scale: Scale) -> String {
    let iv = scale.heptatonic().intervals();
    let rel = (chord.root + 12 - root % 12) % 12;
    let (degree, acc) = match iv.iter().position(|&i| i == rel) {
        Some(d) => (d, ""),
        None => match iv.iter().position(|&i| i == (rel + 1) % 12) {
            Some(d) => (d, "b"),
            None => (iv.iter().position(|&i| i == (rel + 11) % 12).unwrap_or(0), "#"),
        },
    };
    let minorish = matches!(chord.quality, Quality::Minor | Quality::Min7 | Quality::Dim);
    let n = if minorish {
        ROMAN[degree].to_string()
    } else {
        ROMAN[degree].to_ascii_uppercase()
    };
    let suffix = match chord.quality {
        Quality::Dim => "°",
        Quality::Aug => "+",
        Quality::Dom7 | Quality::Min7 => "7",
        Quality::Maj7 => "maj7",
        Quality::Sus2 => "sus2",
        Quality::Sus4 => "sus4",
        Quality::Power => "5",
        _ => "",
    };
    format!("{acc}{n}{suffix}")
}

/// A note of the scale on the neck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FretNote {
    /// 0 = the lowest string.
    pub string: usize,
    pub fret: u8,
    pub midi: u8,
    pub pc: u8,
    /// Which note of the scale (0 = the root).
    pub degree: usize,
}

impl FretNote {
    pub fn is_root(&self) -> bool {
        self.degree == 0
    }
}

/// Standard tuning's open strings, lowest first (E2 A2 D3 G3 B3 E4).
pub const STANDARD: [u8; 6] = [40, 45, 50, 55, 59, 64];

/// Every place on the neck (frets 0..=`frets`) where the scale's notes are, string by string from the lowest
/// (`tuning`: each open string's MIDI note, lowest first: a bass's four, a drop tuning, a capo added in).
pub fn fretboard(tuning: &[u8], frets: u8, root: u8, scale: Scale) -> Vec<FretNote> {
    let iv = scale.intervals();
    let mut out = Vec::new();
    for (string, &open) in tuning.iter().enumerate() {
        for fret in 0..=frets {
            let midi = open + fret;
            let pc = midi % 12;
            let rel = (pc + 12 - root % 12) % 12;
            if let Some(degree) = iv.iter().position(|&i| i == rel) {
                out.push(FretNote {
                    string,
                    fret,
                    midi,
                    pc,
                    degree,
                });
            }
        }
    }
    out
}

/// Whether a played note (MIDI or any octave) is in the scale.
pub fn fits(midi: u8, root: u8, scale: Scale) -> bool {
    let rel = (midi % 12 + 12 - root % 12) % 12;
    scale.intervals().contains(&rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minor_pentatonic_on_the_neck() {
        let a = parse_note("A").unwrap();
        assert_eq!(scale_notes(a, Scale::MinorPentatonic), vec![9, 0, 2, 4, 7]);
        let map = fretboard(&STANDARD, 15, a, Scale::MinorPentatonic);
        // box 1 at the 5th fret: low E has A (5) and C (8), the A string D (5) and E (7)
        let on = |s: usize, f: u8| map.iter().any(|n| n.string == s && n.fret == f);
        assert!(on(0, 5) && on(0, 8) && on(1, 5) && on(1, 7) && on(2, 5) && on(2, 7));
        assert!(!on(0, 6), "Bb isn't in A minor pentatonic");
        let roots: Vec<_> = map
            .iter()
            .filter(|n| n.is_root())
            .map(|n| (n.string, n.fret))
            .collect();
        assert!(roots.contains(&(0, 5)) && roots.contains(&(3, 2)) && roots.contains(&(5, 5)));
        // five notes per octave on every string: 15 frets hold 6 or 7 of them
        for s in 0..6 {
            let n = map.iter().filter(|n| n.string == s).count();
            assert!((6..=8).contains(&n), "string {s}: {n}");
        }
        assert!(fits(69, a, Scale::MinorPentatonic) && !fits(71, a, Scale::MinorPentatonic));
    }

    #[test]
    fn chords_by_name_and_numeral() {
        let c = parse_note("C").unwrap();
        let prog = parse_progression("I-V-vi-IV", c, Scale::Major).unwrap();
        let names: Vec<_> = prog.iter().map(|c| c.name()).collect();
        assert_eq!(names, ["C", "G", "Am", "F"]);
        let a = parse_note("A").unwrap();
        let prog = parse_progression("Am F C G", a, Scale::NaturalMinor).unwrap();
        let romans: Vec<_> = prog
            .iter()
            .map(|ch| roman_of(ch, a, Scale::NaturalMinor))
            .collect();
        assert_eq!(romans, ["i", "VI", "III", "VII"]);
        let same = parse_progression("i – VI – III – VII", a, Scale::MinorPentatonic).unwrap();
        assert_eq!(same, prog);
        let e = parse_note("E").unwrap();
        let mixo = parse_progression("I bVII IV", e, Scale::Major).unwrap();
        assert_eq!(mixo[1].name(), "D");
        assert_eq!(parse_chord("F#m7").unwrap().tones(), vec![6, 9, 1, 4]);
        assert_eq!(parse_chord("Bbm").unwrap().name(), "Bbm");
        assert_eq!(parse_chord("Bdim").unwrap().quality, Quality::Dim);
        assert!(parse_progression("Am H7", a, Scale::Major).is_err());
        assert_eq!(diatonic_triad(c, Scale::Major, 6).name(), "Bdim");
        assert_eq!(parse_roman("V7", a, Scale::HarmonicMinor).unwrap().name(), "E7");
    }

    #[test]
    fn voicings_sit_in_the_middle() {
        for name in ["C", "F", "G", "Am", "Bb", "E"] {
            let v = parse_chord(name).unwrap().voicing(52);
            assert!(v[0] >= 47 && v[0] < 60, "{name}: {v:?}");
            assert_eq!(v[0] % 12, parse_chord(name).unwrap().root);
        }
    }
}
