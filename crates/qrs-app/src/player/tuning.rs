//! The tuner's sums (P37.6b, D136; new, no Python source): what a reading says against the strings it tunes to,
//! measured as the header's "you:" readout is (cents to the nearest note), and the card that offers to shift a song
//! in another standard tuning into E's instead of retuning (never tuning up past standard).

use qrs_formats::model::tuning_name;

/// A tuner reading against the target strings.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The string it's tuning (index into the targets, low first), when within 2.5 semitones of one.
    pub string: Option<usize>,
    /// Cents to the nearest note (what the header shows, and the needle within half a semitone of the string).
    pub cents: f64,
    /// Whole semitones from the string when more than half a semitone off it (0: on its note).
    pub semis: i32,
}

/// The reading for `midi` (fractional) against the open strings `targets`: a string within half a semitone; else
/// one an octave under it (P48.1, the bassist's review: the 12th-fret harmonic, or a reading an octave up, landed on
/// the string two semitones off it, E2 on a bass's D string, and said "tune down" there); else the nearest within
/// 2.5 semitones.
pub fn read(midi: f64, targets: &[i32]) -> Reading {
    let cents = (midi - midi.round()) * 100.0;
    let nearest = |off: f64| {
        (0..targets.len())
            .map(|i| (i, midi - off - targets[i] as f64))
            .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
    };
    let pick = nearest(0.0)
        .filter(|&(_, d)| d.abs() < 0.5)
        .or_else(|| nearest(12.0).filter(|&(_, d)| d.abs() < 0.5))
        .or_else(|| nearest(0.0).filter(|&(_, d)| d.abs() <= 2.5));
    let Some((i, d)) = pick else {
        return Reading {
            string: None,
            cents,
            semis: 0,
        };
    };
    Reading {
        string: Some(i),
        // within half a semitone the nearest note is the string's, so the two readouts are the same number
        cents,
        semis: if d.abs() < 0.5 { 0 } else { d.round() as i32 },
    }
}

/// The reading against string `i` alone, however far off it (P48.1, both reviews: a string clicked in the tuner;
/// retuning E to C, four semitones, was "not near a string", and close strings took each other's readings).
pub fn read_string(midi: f64, targets: &[i32], i: usize) -> Reading {
    let Some(&t) = targets.get(i) else {
        return read(midi, targets);
    };
    let d = midi - t as f64;
    Reading {
        string: Some(i),
        cents: (midi - midi.round()) * 100.0,
        semis: if d.abs() < 0.5 { 0 } else { d.round() as i32 },
    }
}

impl Reading {
    /// The words under the needle: "+3¢ · in tune", "−12¢ · tune up", "2 semitones sharp · tune down" (against the lit string).
    /// `no_tune_up`: the strings are above standard (a song shifted up, or charted up): a reading below one says to
    /// shift the song rather than tune up.
    pub fn verdict(&self, no_tune_up: bool) -> String {
        if self.string.is_none() {
            return "not near a string".into();
        }
        if self.semis != 0 {
            let n = self.semis.unsigned_abs();
            let what = if n == 1 { "semitone" } else { "semitones" };
            // (the string is lit above the dial: "sharp" / "flat" keeps the line short enough for a narrow tile)
            let side = if self.semis > 0 { "sharp" } else { "flat" };
            let then = if self.semis > 0 {
                "tune down"
            } else if no_tune_up {
                "shift the song"
            } else {
                "tune up"
            };
            return format!("{n} {what} {side}  ·  {then}");
        }
        let then = if self.cents.abs() <= 5.0 {
            "in tune"
        } else if self.cents < 0.0 {
            if no_tune_up { "shift the song" } else { "tune up" }
        } else {
            "tune down"
        };
        format!("{:+.0}¢  ·  {then}", self.cents)
    }

    /// The needle's cents: the reading's within half a semitone of the string, else pinned to the side it's off.
    pub fn needle(&self) -> f64 {
        match self.semis {
            0 => self.cents,
            s if s > 0 => 50.0,
            _ => -50.0,
        }
    }
}

/// The tuning card's offer: the song (as it sounds with `pitch`) is in a standard or drop tuning `k` semitones from
/// E's; playing it at `to_pitch` puts it in E's (`then`) on a guitar in standard.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub k: i32,
    pub to_pitch: i32,
    /// The tuning it sounds in now ("Eb Standard"), and with the offer taken ("E Standard").
    pub now: String,
    pub then: String,
}

/// The offer for a part tuned `offsets` (per string, from E standard) heard with `pitch`, or None when it's in E
/// already, isn't a standard or drop tuning (an open tuning is retuned for), or the shift would go past an octave.
pub fn offer(offsets: &[i32], is_bass: bool, pitch: i32, max: i32) -> Option<Offer> {
    let n = if is_bass { 4 } else { 6 };
    let mut o: Vec<i32> = offsets.to_vec();
    o.resize(o.len().max(n), 0);
    o.truncate(n);
    let k = o[1] + pitch;
    let standard = o.iter().all(|&x| x == o[1]);
    let drop = o[0] == o[1] - 2 && o[1..].iter().all(|&x| x == o[1]);
    if !(standard || drop) || k == 0 {
        return None;
    }
    let to_pitch = pitch - k;
    if to_pitch.abs() > max {
        return None;
    }
    let sounding = |p: i32| -> Vec<i32> { o.iter().map(|x| x + p).collect() };
    Some(Offer {
        k,
        to_pitch,
        now: tuning_name(&sounding(pitch), is_bass),
        then: tuning_name(&sounding(to_pitch), is_bass),
    })
}

impl Offer {
    /// The card's line: below standard, retune or shift; above, shift (never tune up past standard).
    pub fn words(&self) -> String {
        let n = self.k.unsigned_abs();
        let how = if n == 1 {
            "a semitone".to_string()
        } else {
            format!("{n} semitones")
        };
        if self.k < 0 {
            format!(
                "The song is in {}, {how} below standard. Tune down to it, or play it in {} as you're tuned now.",
                self.now, self.then
            )
        } else {
            format!(
                "The song sounds in {}, {how} above standard. Don't tune up: play it in {} instead.",
                self.now, self.then
            )
        }
    }

    /// The card's key: "Play in E Standard (song +1)", "Play in E Standard (original pitch)".
    pub fn key(&self) -> String {
        if self.to_pitch == 0 {
            format!("Play in {} (original pitch)", self.then)
        } else {
            format!(
                "Play in {} (song {})",
                self.then,
                super::songaudio::pitch_short(self.to_pitch)
            )
        }
    }
}

/// Cents off (the median of what's played) that the out-of-tune warning is given at. Picked notes read sharper than
/// the guitar is tuned, from the attack and most on low strings: one player's in-tune takes read up to +29, and a
/// warning at 20 would have come on 7 of 11 of them. On the synth playing the Boston lead with the whole guitar
/// detuned, up to 30 cents costs about 3% of single notes, 35 sharp 12%, 40 over half.
pub const DRIFT_WARN: f64 = 30.0;
/// A reading this far (semitones) from the nearest note charted around now is a wrong note, not the guitar's
/// tuning. Wider than half a semitone: a guitar 35 cents sharp reads 50-70 sharp on picked notes.
pub const DRIFT_MATCH: f64 = 0.8;
/// Different chart notes the playing must have been heard on before the warning can be given.
pub const DRIFT_NOTES: usize = 8;

/// How far out of tune the guitar sounds, from what's heard while a song plays (a tuner reading near a note the
/// chart has just then). The median, so a wrong note or a slip doesn't move it.
#[derive(Clone, Debug, Default)]
pub struct Drift {
    /// The latest readings' cents from the note they matched.
    cents: std::collections::VecDeque<f64>,
    /// The chart notes they matched (group indices), for "heard on enough different notes".
    notes: std::collections::VecDeque<usize>,
}

impl Drift {
    const KEEP: usize = 60;

    /// A tuner reading `midi` (fractional) while `expected` (group index, the pitch it sounds) are the notes near
    /// now: kept when it's within `DRIFT_MATCH` of one of them.
    pub fn hear(&mut self, midi: f64, expected: &[(usize, i32)]) {
        let Some(&(gi, m)) = expected
            .iter()
            .min_by(|a, b| (midi - a.1 as f64).abs().total_cmp(&(midi - b.1 as f64).abs()))
        else {
            return;
        };
        let d = midi - m as f64;
        if d.abs() >= DRIFT_MATCH {
            return;
        }
        self.cents.push_back(d * 100.0);
        self.notes.push_back(gi);
        if self.cents.len() > Self::KEEP {
            self.cents.pop_front();
            self.notes.pop_front();
        }
    }

    /// The guitar's cents off (+ sharp), once it's been heard on `DRIFT_NOTES` different notes.
    pub fn offset(&self) -> Option<f64> {
        let distinct: std::collections::BTreeSet<usize> = self.notes.iter().copied().collect();
        if distinct.len() < DRIFT_NOTES {
            return None;
        }
        let mut v: Vec<f64> = self.cents.iter().copied().collect();
        v.sort_by(|a, b| a.total_cmp(b));
        Some(v[v.len() / 2])
    }

    /// The warning's words, when the guitar is `DRIFT_WARN` or more out.
    pub fn warning(&self) -> Option<(String, String)> {
        let c = self.offset()?;
        if c.abs() < DRIFT_WARN {
            return None;
        }
        let (side, then) = if c > 0.0 { ("sharp", "tune down") } else { ("flat", "tune up") };
        Some((
            format!("Your guitar sounds {:.0}\u{a2} {side}", c.abs()),
            format!("  \u{b7}  {then} a little (the Tuner tile)"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guitar_far_out_of_tune_is_warned_and_one_near_in_tune_is_not() {
        // eight notes each heard 35 cents sharp, a wrong note and a slip among them
        let mut d = Drift::default();
        for i in 0..7 {
            d.hear(50.0 + i as f64 + 0.35, &[(i, 50 + i as i32)]);
        }
        assert_eq!(d.offset(), None, "seven notes aren't enough");
        d.hear(59.0, &[(7, 57)]); // a wrong note: not near the chart's, not kept
        assert_eq!(d.offset(), None);
        d.hear(57.36, &[(7, 57)]);
        d.hear(57.02, &[(7, 57)]); // a slip
        let c = d.offset().unwrap();
        assert!((c - 35.0).abs() < 1.5, "{c}");
        let (text, tail) = d.warning().unwrap();
        assert_eq!(text, "Your guitar sounds 35\u{a2} sharp");
        assert!(tail.contains("tune down"), "{tail}");
        // a guitar 35 cents sharp whose picked notes read 55-70 sharp: still measured, not taken for wrong notes
        let mut d = Drift::default();
        for i in 0..10 {
            d.hear(45.0 + i as f64 + 0.55 + 0.015 * i as f64, &[(i, 45 + i as i32)]);
        }
        assert!(d.offset().is_some_and(|c| c > 55.0));
        // 25 cents sharp (what hard-picked notes read on a guitar in tune): no warning (a chord's notes are all
        // candidates)
        let mut d = Drift::default();
        for i in 0..10 {
            d.hear(45.0 + i as f64 + 0.25, &[(i, 40), (i, 45 + i as i32)]);
        }
        assert!(d.offset().is_some());
        assert_eq!(d.warning(), None);
        // flat
        let mut d = Drift::default();
        for i in 0..10 {
            d.hear(45.0 + i as f64 - 0.38, &[(i, 45 + i as i32)]);
        }
        let (text, tail) = d.warning().unwrap();
        assert_eq!(text, "Your guitar sounds 38\u{a2} flat");
        assert!(tail.contains("tune up"), "{tail}");
        // just past the line, flat: the whole toast
        let mut d = Drift::default();
        for i in 0..10 {
            d.hear(45.0 + i as f64 - 0.32, &[(i, 45 + i as i32)]);
        }
        let (text, tail) = d.warning().unwrap();
        assert_eq!(
            format!("{text}{tail}"),
            "Your guitar sounds 32\u{a2} flat  \u{b7}  tune up a little (the Tuner tile)"
        );
    }

    // E standard's open strings, and Eb standard's
    const E: [i32; 6] = [40, 45, 50, 55, 59, 64];
    const EB: [i32; 6] = [39, 44, 49, 54, 58, 63];

    #[test]
    fn the_tuner_and_the_header_measure_alike() {
        // the review's case: Eb3 +3 cents with the song in Eb standard: the low Eb an octave up (P48.1: a harmonic
        // or an octave reading is its own string), and clicked onto the C#3 string, 2 semitones above that one
        let m = 51.03;
        let r = read(m, &EB);
        assert_eq!((r.string, r.semis), (Some(0), 0));
        let r = read_string(m, &EB, 2);
        assert_eq!(r.string, Some(2));
        assert_eq!(r.semis, 2);
        assert!((r.cents - 3.0).abs() < 1e-6, "{}", r.cents); // as the header's "you: Eb3 +3¢"
        let v = r.verdict(false);
        assert_eq!(v, "2 semitones sharp  ·  tune down");
        assert!(!v.contains("203"));
        assert_eq!(r.needle(), 50.0);
        // close to a string: cents to the string, which is the nearest note
        let r = read(49.12, &EB);
        assert_eq!((r.string, r.semis), (Some(2), 0));
        assert!((r.cents - 12.0).abs() < 1e-6);
        assert_eq!(r.verdict(false), "+12¢  ·  tune down");
        assert_eq!(read(48.97, &EB).verdict(false), "-3¢  ·  in tune");
        // more than half a semitone flat: rounds to the semitone it's nearer
        let r = read(48.4, &EB);
        assert_eq!(r.semis, -1);
        assert!((r.cents - 40.0).abs() < 1e-6); // (48.4 is G#2/Ab2 +40¢)
        assert_eq!(r.verdict(false), "1 semitone flat  ·  tune up");
        assert_eq!(r.verdict(true), "1 semitone flat  ·  shift the song");
        // nowhere near a string
        assert_eq!(read(70.0, &E).string, None);
    }

    #[test]
    fn a_song_below_standard_offers_a_shift_up_and_never_a_tune_up() {
        // Eb standard: tune down, or play it a semitone up in E standard
        let o = offer(&[-1; 6], false, 0, 12).unwrap();
        assert_eq!((o.k, o.to_pitch), (-1, 1));
        assert_eq!((o.now.as_str(), o.then.as_str()), ("Eb Standard", "E Standard"));
        assert_eq!(o.key(), "Play in E Standard (song +1)");
        assert!(
            o.words()
                .starts_with("The song is in Eb Standard, a semitone below standard. Tune down")
        );
        // Drop C#: shift +1 to Drop D
        let o = offer(&[-3, -1, -1, -1, -1, -1], false, 0, 12).unwrap();
        assert_eq!((o.to_pitch, o.then.as_str()), (1, "Drop D"));
        // the song shifted up 2 from E standard: it sounds in F#... never "tune up", back to the original pitch
        let o = offer(&[0; 6], false, 2, 12).unwrap();
        assert_eq!(o.k, 2);
        assert_eq!(o.to_pitch, 0);
        assert!(o.words().contains("Don't tune up"), "{}", o.words());
        assert_eq!(o.key(), "Play in E Standard (original pitch)");
        // already in E (Eb shifted +1), or E standard, or an open tuning: no card
        assert_eq!(offer(&[-1; 6], false, 1, 12), None);
        assert_eq!(offer(&[0; 6], false, 0, 12), None);
        assert_eq!(offer(&[-2, -2, 0, 0, 0, -2], false, 0, 12), None);
        // bass: its four strings
        let o = offer(&[-2, -2, -2, -2], true, 0, 12).unwrap();
        assert_eq!((o.to_pitch, o.then.as_str()), (2, "E Standard"));
        // past the shift's reach
        assert_eq!(offer(&[-1; 6], false, 0, 0), None);
    }

    #[test]
    fn a_harmonic_or_an_octave_up_is_its_own_string() {
        // a bass's E at the 12th fret (E2, 40) was read as its D string (38) two semitones sharp
        let bass = [28, 33, 38, 43];
        let r = read(40.1, &bass);
        assert_eq!((r.string, r.semis), (Some(0), 0));
        assert!((r.cents - 10.0).abs() < 1e-6);
        // a guitar's low E heard an octave up (E3, 52) is the low E, not the D string
        let gtr = [40, 45, 50, 55, 59, 64];
        assert_eq!(read(52.0, &gtr).string, Some(0));
        // a string on its own note still wins over an octave match (the high E against the low one)
        assert_eq!(read(64.0, &gtr).string, Some(5));
    }

    #[test]
    fn a_clicked_string_is_read_however_far_off() {
        // retuning E2 down to C2: four semitones, "not near a string" by itself
        let to_c = [36, 45, 50, 55, 59, 64];
        assert_eq!(read(40.0, &to_c).string, None);
        let r = read_string(40.0, &to_c, 0);
        assert_eq!((r.string, r.semis), (Some(0), 4));
        assert_eq!(r.verdict(false), "4 semitones sharp  ·  tune down");
        // DADGAD's G and A strings: a sharp G is the G's, not the A's, once the G is clicked
        let dadgad = [38, 45, 50, 55, 57, 62];
        let r = read_string(56.2, &dadgad, 3);
        assert_eq!((r.string, r.semis), (Some(3), 1));
        // nothing near: no number that looks like a string's cents
        assert_eq!(read(70.0, &[40]).verdict(false), "not near a string");
    }
}
