//! The instrument an input plays (P32, D89; new in quickRS, no Python source): a guitar, as every code path assumed
//! before, or a bass with 4 or 5 strings. Set per input in Settings > Audio (config.json `input_cal[id].instrument`).
//!
//! Guitar keeps every number it had: `pitch_for(Guitar, ..)` is `pitch::pitch` and `Guitar.pitch_window` is the
//! 4096 samples the tuner always read. A bass reads lower (down to 28 Hz, under a 5-string's low B at 30.9 Hz), over
//! a longer window (two periods of a low B need 65 ms; 8192 samples at 48 kHz hold five), and asks more of the first
//! peak (k = 0.95: a bass's 2nd harmonic is often louder than its fundamental, and the guitar's 0.9 takes it for the
//! note an octave up).

use qrs_formats::model::GUITAR_OPEN_MIDI;

use crate::pitch::{mpm_pitch, pitch};
use crate::spectrum::Spectra;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Instrument {
    #[default]
    Guitar,
    /// A 4-string bass (E1 A1 D2 G2).
    Bass,
    /// A 5-string bass (B0 E1 A1 D2 G2).
    Bass5,
}

/// A 4-string bass's open strings in standard tuning (MIDI).
pub const BASS4: [i32; 4] = [28, 33, 38, 43];
/// A 5-string bass's: a low B under the E.
pub const BASS5: [i32; 5] = [23, 28, 33, 38, 43];

impl Instrument {
    pub const ALL: [Instrument; 3] = [Instrument::Guitar, Instrument::Bass, Instrument::Bass5];

    /// Its key in config.json.
    pub fn key(self) -> &'static str {
        match self {
            Instrument::Guitar => "guitar",
            Instrument::Bass => "bass",
            Instrument::Bass5 => "bass5",
        }
    }

    /// From config.json (anything else, or nothing, is a guitar).
    pub fn parse(s: &str) -> Instrument {
        match s {
            "bass" => Instrument::Bass,
            "bass5" => Instrument::Bass5,
            _ => Instrument::Guitar,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Instrument::Guitar => "Guitar",
            Instrument::Bass => "Bass",
            Instrument::Bass5 => "5-string bass",
        }
    }

    pub fn is_bass(self) -> bool {
        self != Instrument::Guitar
    }

    /// What the player's instrument is called in a sentence (P37.6a, D131): "guitar" or "bass" (a 5-string is a
    /// bass too); `NEUTRAL` is the word for either.
    pub fn noun(self) -> &'static str {
        if self.is_bass() { "bass" } else { "guitar" }
    }

    /// `noun` at the start of a sentence or a label ("Guitar", "Bass").
    pub fn noun_cap(self) -> &'static str {
        if self.is_bass() { "Bass" } else { "Guitar" }
    }

    /// `noun` in a caps heading ("GUITAR", "BASS").
    pub fn noun_upper(self) -> &'static str {
        if self.is_bass() { "BASS" } else { "GUITAR" }
    }

    /// The word for either instrument, where a line doesn't know which.
    pub const NEUTRAL: &'static str = "instrument";

    /// How a note is struck on it, in a sentence: "strum" on a guitar, "pluck" on a bass.
    pub fn strike(self) -> &'static str {
        if self.is_bass() { "pluck" } else { "strum" }
    }

    /// `strike` at the start of a sentence ("Strum", "Pluck").
    pub fn strike_cap(self) -> &'static str {
        if self.is_bass() { "Pluck" } else { "Strum" }
    }

    /// A line written for a guitar, said for this instrument: on a bass "guitar" is "bass" and "strum" is "pluck"
    /// (any case, plurals and "-ming" / "-med" forms); on a guitar it comes back as it is.
    pub fn say(self, line: &str) -> String {
        if !self.is_bass() {
            return line.to_string();
        }
        const SWAPS: [(&str, &str); 18] = [
            ("guitars", "basses"),
            ("Guitars", "Basses"),
            ("GUITARS", "BASSES"),
            ("guitar", "bass"),
            ("Guitar", "Bass"),
            ("GUITAR", "BASS"),
            ("strumming", "plucking"),
            ("Strumming", "Plucking"),
            ("STRUMMING", "PLUCKING"),
            ("strummed", "plucked"),
            ("Strummed", "Plucked"),
            ("STRUMMED", "PLUCKED"),
            ("strum", "pluck"),
            ("Strum", "Pluck"),
            ("STRUM", "PLUCK"),
            // ("a pluck" stays "a pluck"; a guitar's "a muted chord" is a bass's "a muted note")
            ("muted chords", "muted notes"),
            ("a muted chord", "a muted note"),
            ("a few chords", "a few notes"),
        ];
        let mut s = line.to_string();
        for (a, b) in SWAPS {
            if s.contains(a) {
                s = s.replace(a, b);
            }
        }
        s
    }

    /// Its open strings in standard tuning, low to high (MIDI).
    pub fn open_strings(self) -> Vec<i32> {
        match self {
            Instrument::Guitar => GUITAR_OPEN_MIDI.to_vec(),
            Instrument::Bass => BASS4.to_vec(),
            Instrument::Bass5 => BASS5.to_vec(),
        }
    }

    /// The tuner's pitch range (Hz).
    pub fn pitch_range(self) -> (f64, f64) {
        if self.is_bass() {
            (28.0, 1000.0)
        } else {
            (35.0, 1400.0)
        }
    }

    /// How many samples the tuner and the note readout analyse.
    pub fn pitch_window(self, rate: u32) -> usize {
        if self.is_bass() {
            ((rate as f64 * 0.17) as usize).next_power_of_two().max(4096)
        } else {
            4096
        }
    }

    /// Open strings with one more on a 5-string: a part's open strings (low to high) with a low B (a fourth under
    /// its lowest) added when the instrument has five strings and the part four.
    pub fn strings_for(self, part_opens: &[i32]) -> Vec<i32> {
        let mut v = part_opens.to_vec();
        if self == Instrument::Bass5 && v.len() == 4 {
            v.insert(0, v[0] - 5);
        }
        v
    }
}

/// The tuner's pitch (Hz, clarity) for this instrument: `pitch::pitch` for a guitar, unchanged.
pub fn pitch_for(inst: Instrument, sp: &mut Spectra, x: &[f32], rate: u32) -> (f64, f64) {
    if inst.is_bass() {
        let (lo, hi) = inst.pitch_range();
        mpm_pitch(sp, x, rate, lo, hi, 0.95)
    } else {
        pitch(sp, x, rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_instrument_has_its_own_words() {
        assert_eq!(Instrument::Guitar.noun(), "guitar");
        assert_eq!(Instrument::Bass.noun(), "bass");
        assert_eq!(Instrument::Bass5.noun_cap(), "Bass");
        assert_eq!(Instrument::Bass5.noun_upper(), "BASS");
        assert_eq!(Instrument::Guitar.strike(), "strum");
        assert_eq!(Instrument::Bass.strike_cap(), "Pluck");
        let line = "Strum a muted chord: YOUR GUITAR's strums, strumming and strummed guitars.";
        assert_eq!(Instrument::Guitar.say(line), line);
        assert_eq!(
            Instrument::Bass5.say(line),
            "Pluck a muted note: YOUR BASS's plucks, plucking and plucked basses."
        );
        // "bass" and "pluck" stay as they are
        assert_eq!(Instrument::Bass.say("a bass pluck"), "a bass pluck");
    }
    use crate::pitch::{hz_to_midi, midi_to_hz};
    use crate::testsig;

    fn cents(got: f64, midi: f64) -> f64 {
        1200.0 * (got / midi_to_hz(midi)).log2()
    }

    #[test]
    fn keys_round_trip_and_default_to_guitar() {
        for i in Instrument::ALL {
            assert_eq!(Instrument::parse(i.key()), i);
        }
        assert_eq!(Instrument::parse(""), Instrument::Guitar);
        assert_eq!(Instrument::parse("ukulele"), Instrument::Guitar);
        assert_eq!(Instrument::default(), Instrument::Guitar);
        assert_eq!(
            Instrument::Bass5.strings_for(&[28, 33, 38, 43]),
            vec![23, 28, 33, 38, 43]
        );
        assert_eq!(
            Instrument::Bass.strings_for(&[28, 33, 38, 43]),
            vec![28, 33, 38, 43]
        );
        assert_eq!(Instrument::Guitar.pitch_window(48000), 4096);
        assert_eq!(Instrument::Bass.pitch_window(48000), 8192);
    }

    /// A guitar reads exactly as before (the same function, the same numbers, bit for bit).
    #[test]
    fn guitar_pitch_is_unchanged() {
        let rate = 48000;
        let mut sp = Spectra::new();
        for m in [40.0, 45.0, 52.0, 59.0, 64.0, 76.0] {
            let x = testsig::ks_pluck(m, rate, 0.4, 0.3, 0.3, 3);
            let w = &x[2400..2400 + 4096];
            assert_eq!(
                pitch_for(Instrument::Guitar, &mut sp, w, rate),
                pitch(&mut sp, w, rate)
            );
        }
    }

    /// Plucked bass notes, down to a 5-string's low B, at soft and loud levels, a Karplus-Strong string and one
    /// whose 2nd harmonic is three times its fundamental: read within 5 cents in bass mode. The guitar's reading
    /// stops at 35 Hz and takes the weak-fundamental low B an octave up.
    #[test]
    fn bass_notes_are_read_down_to_a_low_b() {
        let rate = 48000;
        let mut sp = Spectra::new();
        let n = Instrument::Bass5.pitch_window(rate);
        let mut guitar_wrong = 0;
        for &m in &[23.0, 28.0, 33.0, 38.0, 43.0, 50.0, 55.0] {
            for &amp in &[0.01f32, 0.1, 0.6] {
                for weak in [false, true] {
                    let x = if weak {
                        testsig::additive_pluck(m, rate, 1.0, amp, &[0.3, 1.0, 0.5, 0.3, 0.15], 0.008, 1.5)
                    } else {
                        testsig::ks_pluck(m, rate, 1.0, amp, 0.6, m as u32)
                    };
                    let w = &x[4800..4800 + n];
                    let (hz, clarity) = pitch_for(Instrument::Bass5, &mut sp, w, rate);
                    assert!(
                        cents(hz, m).abs() < 5.0,
                        "{m} {amp} weak={weak}: {hz} Hz ({:.1})",
                        hz_to_midi(hz)
                    );
                    assert!(clarity > 0.85, "{m} {amp} weak={weak}: clarity {clarity}");
                    let (g, _) = pitch(&mut sp, &w[..4096], rate);
                    if g <= 0.0 || cents(g, m).abs() > 50.0 {
                        guitar_wrong += 1;
                    }
                }
            }
        }
        // the guitar's reading takes the weak-fundamental low B an octave up (the three levels), and finds the
        // other only at the edge of its range (31.0 Hz)
        assert!(guitar_wrong >= 3, "{guitar_wrong}");
    }
}
