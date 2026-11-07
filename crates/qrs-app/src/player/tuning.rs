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

/// The reading for `midi` (fractional) against the open strings `targets`.
pub fn read(midi: f64, targets: &[i32]) -> Reading {
    let cents = (midi - midi.round()) * 100.0;
    let near = (0..targets.len()).min_by(|&a, &b| {
        (targets[a] as f64 - midi)
            .abs()
            .total_cmp(&(targets[b] as f64 - midi).abs())
    });
    let Some(i) = near.filter(|&i| (targets[i] as f64 - midi).abs() <= 2.5) else {
        return Reading {
            string: None,
            cents,
            semis: 0,
        };
    };
    let d = midi - targets[i] as f64;
    Reading {
        string: Some(i),
        // within half a semitone the nearest note is the string's, so the two readouts are the same number
        cents,
        semis: if d.abs() < 0.5 { 0 } else { d.round() as i32 },
    }
}

impl Reading {
    /// The words under the needle: "+3¢ · in tune", "−12¢ · tune up", "2 semitones sharp · tune down" (against the lit string).
    /// `no_tune_up`: the guitar's target is above standard: a reading below the target says to shift the guitar input
    /// rather than tune up.
    pub fn verdict(&self, no_tune_up: bool) -> String {
        if self.string.is_none() {
            return format!("{:+.0}¢  ·  not near a string", self.cents);
        }
        if self.semis != 0 {
            let n = self.semis.unsigned_abs();
            let what = if n == 1 { "semitone" } else { "semitones" };
            // (the string is lit above the dial: "sharp" / "flat" keeps the line short enough for a narrow tile)
            let side = if self.semis > 0 { "sharp" } else { "flat" };
            let then = if self.semis > 0 {
                "tune down"
            } else if no_tune_up {
                "shift the guitar input"
            } else {
                "tune up"
            };
            return format!("{n} {what} {side}  ·  {then}");
        }
        let then = if self.cents.abs() <= 5.0 {
            "in tune"
        } else if self.cents < 0.0 {
            if no_tune_up {
                "shift the guitar input"
            } else {
                "tune up"
            }
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

/// The tuning card's offer: after the input shift, the guitar's target tuning is `k` semitones from E's; `to_pitch`
/// shifts the input so the guitar can stay in E standard (`then`).
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub k: i32,
    pub to_pitch: i32,
    /// The physical guitar tuning now ("Eb Standard"), and with the offer taken ("E Standard").
    pub now: String,
    pub then: String,
}

/// The offer for a part tuned `offsets` (per string, from E standard) with input shift `pitch`, or None when it's
/// already E standard, isn't a standard or drop tuning (an open tuning is retuned for), or the shift would go past
/// an octave.
pub fn offer(offsets: &[i32], is_bass: bool, pitch: i32, max: i32) -> Option<Offer> {
    let n = if is_bass { 4 } else { 6 };
    let mut o: Vec<i32> = offsets.to_vec();
    o.resize(o.len().max(n), 0);
    o.truncate(n);
    // The raw guitar tuning that lands on the chart after the input shift is chart_tuning - pitch.
    let k = o[1] - pitch;
    let standard = o.iter().all(|&x| x == o[1]);
    let drop = o[0] == o[1] - 2 && o[1..].iter().all(|&x| x == o[1]);
    if !(standard || drop) || k == 0 {
        return None;
    }
    let to_pitch = o[1];
    if to_pitch.abs() > max {
        return None;
    }
    let guitar_tuning = |p: i32| -> Vec<i32> { o.iter().map(|x| x - p).collect() };
    Some(Offer {
        k,
        to_pitch,
        now: tuning_name(&guitar_tuning(pitch), is_bass),
        then: tuning_name(&guitar_tuning(to_pitch), is_bass),
    })
}

impl Offer {
    /// The card's line: below standard, retune or shift the input down; above, shift the input up.
    pub fn words(&self) -> String {
        let n = self.k.unsigned_abs();
        let how = if n == 1 {
            "a semitone".to_string()
        } else {
            format!("{n} semitones")
        };
        if self.k < 0 {
            format!(
                "The track is in {}, {how} below standard. Tune down to it, or shift the guitar input down to play in {}.",
                self.now, self.then
            )
        } else {
            format!(
                "The track is in {}, {how} above standard. Tune up to it, or shift the guitar input up to play in {}.",
                self.now, self.then
            )
        }
    }

    /// The card's key: "Play in E Standard (input −1)", "Play in E Standard (no shift)".
    pub fn key(&self) -> String {
        if self.to_pitch == 0 {
            format!("Play in {} (no shift)", self.then)
        } else {
            format!(
                "Play in {} (input {})",
                self.then,
                super::songaudio::pitch_short(self.to_pitch)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // E standard's open strings, and Eb standard's
    const E: [i32; 6] = [40, 45, 50, 55, 59, 64];
    const EB: [i32; 6] = [39, 44, 49, 54, 58, 63];

    #[test]
    fn the_tuner_and_the_header_measure_alike() {
        // the review's case: Eb3 +3 cents with the song in Eb standard (C#3 is the nearest string, 2 below)
        let m = 51.03;
        let r = read(m, &EB);
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
        assert_eq!(r.verdict(true), "1 semitone flat  ·  shift the guitar input");
        // nowhere near a string
        assert_eq!(read(70.0, &E).string, None);
    }

    #[test]
    fn a_track_below_standard_offers_an_input_shift_down_for_standard_tuning() {
        // Eb standard: tune down, or shift guitar input down while playing in E standard
        let o = offer(&[-1; 6], false, 0, 12).unwrap();
        assert_eq!((o.k, o.to_pitch), (-1, -1));
        assert_eq!((o.now.as_str(), o.then.as_str()), ("Eb Standard", "E Standard"));
        assert_eq!(o.key(), "Play in E Standard (input \u{2212}1)");
        assert!(
            o.words()
                .starts_with("The track is in Eb Standard, a semitone below standard. Tune down")
        );
        // Drop C#: shift the guitar input down to play in Drop D
        let o = offer(&[-3, -1, -1, -1, -1, -1], false, 0, 12).unwrap();
        assert_eq!((o.to_pitch, o.then.as_str()), (-1, "Drop D"));
        // An input shifted down two on an E chart can use standard tuning by clearing the shift.
        let o = offer(&[0; 6], false, -2, 12).unwrap();
        assert_eq!(o.k, 2);
        assert_eq!(o.to_pitch, 0);
        assert!(o.words().contains("shift the guitar input up"), "{}", o.words());
        assert_eq!(o.key(), "Play in E Standard (no shift)");
        // Already physically in E, or an open tuning: no card
        assert_eq!(offer(&[-1; 6], false, -1, 12), None);
        assert_eq!(offer(&[0; 6], false, 0, 12), None);
        assert_eq!(offer(&[-2, -2, 0, 0, 0, -2], false, 0, 12), None);
        // bass: its four strings
        let o = offer(&[-2, -2, -2, -2], true, 0, 12).unwrap();
        assert_eq!((o.to_pitch, o.then.as_str()), (-2, "E Standard"));
        // past the shift's reach
        assert_eq!(offer(&[-1; 6], false, 0, 0), None);
    }
}
