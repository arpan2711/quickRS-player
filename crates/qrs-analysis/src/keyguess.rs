//! The key you seem to be playing in: a pitch-class histogram of what you've played recently (older notes fade)
//! scored against the Krumhansl-Kessler major and minor key profiles, with some stickiness so it doesn't flicker.
//! Port of rsplay/notefeed.py `KeyGuess`.

/// Seconds for old notes to count half as much.
pub const FADE_HALF_LIFE: f64 = 20.0;
/// This much playing (seconds of held notes) before guessing.
pub const MIN_WEIGHT: f64 = 3.0;
/// A new key must beat the current one by this much correlation.
pub const SWITCH_MARGIN: f64 = 0.04;

const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Major,
    Minor,
}

/// (tonic pitch class, mode)
pub type Key = (u8, Mode);

fn profile(tonic: usize, mode: Mode) -> [f64; 12] {
    let base = if mode == Mode::Major { MAJOR } else { MINOR };
    // np.roll(p, t): element i moves to i + t
    core::array::from_fn(|i| base[(i + 12 - tonic) % 12])
}

fn corr(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let ma = a.iter().sum::<f64>() / 12.0;
    let mb = b.iter().sum::<f64>() / 12.0;
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        num += (a[i] - ma) * (b[i] - mb);
        da += (a[i] - ma).powi(2);
        db += (b[i] - mb).powi(2);
    }
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da * db).sqrt()
    }
}

/// The best-scoring key for a pitch-class histogram, and its score.
fn best_key(hist: &[f64; 12]) -> (Key, f64) {
    let mut best = ((0, Mode::Major), f64::MIN);
    for mode in [Mode::Major, Mode::Minor] {
        for t in 0..12u8 {
            let s = corr(hist, &profile(t as usize, mode));
            if s > best.1 {
                best = ((t, mode), s);
            }
        }
    }
    best
}

/// A whole song's key from its notes, (MIDI pitch, seconds it sounds), all counted alike (no fading, no stickiness):
/// the note widget's SONG key (owner request). None with under `MIN_WEIGHT` seconds of notes.
pub fn key_of(notes: impl IntoIterator<Item = (i32, f64)>) -> Option<Key> {
    let mut hist = [0.0; 12];
    for (midi, secs) in notes {
        hist[midi.rem_euclid(12) as usize] += secs.max(0.0);
    }
    (hist.iter().sum::<f64>() >= MIN_WEIGHT).then(|| best_key(&hist).0)
}

/// A part's own key: its notes and its chords' notes, each by how long it sounds (an open sustain counts as 0.15 s),
/// at the pitches its tuning plays them.
pub fn song_key(arr: &qrs_formats::model::Arrangement) -> Option<Key> {
    let opens = arr.open_string_midi();
    let notes = arr
        .notes
        .iter()
        .chain(arr.chords.iter().flat_map(|c| c.notes.iter()));
    key_of(notes.map(|n| (crate::detect::note_midi(&opens, n), n.sustain.max(0.15))))
}

#[derive(Clone, Debug)]
pub struct KeyGuess {
    pub hist: [f64; 12],
    pub key: Option<Key>,
    pub confidence: f64,
    last: Option<f64>,
}

impl Default for KeyGuess {
    fn default() -> Self {
        KeyGuess {
            hist: [0.0; 12],
            key: None,
            confidence: 0.0,
            last: None,
        }
    }
}

impl KeyGuess {
    /// The pitch class being played now (None when quiet), at time `now` (seconds).
    pub fn add(&mut self, pc: Option<u8>, now: f64) {
        self.add_all(pc.as_slice(), now);
    }

    /// The pitch classes sounding now (a chord's; none when quiet), sharing the time between them (P25).
    pub fn add_all(&mut self, pcs: &[u8], now: f64) {
        let dt = self.last.map(|l| (now - l).max(0.0)).unwrap_or(0.0);
        self.last = Some(now);
        let fade = 0.5f64.powf(dt / FADE_HALF_LIFE);
        for h in &mut self.hist {
            *h *= fade;
        }
        for &pc in pcs {
            self.hist[(pc % 12) as usize] += dt / pcs.len() as f64;
        }
        if self.hist.iter().sum::<f64>() < MIN_WEIGHT {
            return;
        }
        let (best_key, best_score) = best_key(&self.hist);
        let current_score = self.key.map(|(t, m)| corr(&self.hist, &profile(t as usize, m)));
        match current_score {
            Some(cur) if best_score <= cur + SWITCH_MARGIN => {}
            _ => self.key = Some(best_key),
        }
        let key = self.key.unwrap();
        let s = corr(&self.hist, &profile(key.0 as usize, key.1));
        self.confidence = s.max(0.0);
    }

    pub fn reset(&mut self) {
        self.hist = [0.0; 12];
        self.key = None;
        self.confidence = 0.0;
    }

    pub fn label(&self) -> Option<String> {
        self.key.map(key_label)
    }
}

/// A key's name: "A major", "F# minor".
pub fn key_label((t, m): Key) -> String {
    format!(
        "{} {}",
        qrs_formats::model::note_name(t as i32),
        if m == Mode::Major { "major" } else { "minor" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minor_pentatonic_reads_as_a_minor_or_c_major() {
        let mut k = KeyGuess::default();
        let notes = [9u8, 0, 2, 4, 7, 9, 4, 9, 0, 9];
        let mut t = 0.0;
        for _ in 0..20 {
            for &n in &notes {
                t += 0.3;
                k.add(Some(n), t);
            }
        }
        let label = k.label().unwrap();
        assert!(label == "A minor" || label == "C major", "{label}");
        assert!(k.confidence > 0.5);
    }

    #[test]
    fn a_songs_key_from_its_notes() {
        // a G major tune (G A B C D E F#, G and D held longest), in two octaves
        let tune = [
            (67, 1.0),
            (69, 0.4),
            (71, 0.6),
            (72, 0.4),
            (74, 0.8),
            (76, 0.4),
            (78, 0.3),
        ];
        let notes = tune.iter().chain(&tune).map(|&(m, s)| (m - 12, s));
        assert_eq!(key_of(notes), Some((7, Mode::Major)));
        // E minor: E G B leaned on, D and C passing
        let tune = [
            (64, 1.5),
            (67, 0.8),
            (71, 1.0),
            (62, 0.3),
            (60, 0.3),
            (66, 0.3),
            (69, 0.4),
        ];
        assert_eq!(key_of(tune), Some((4, Mode::Minor)));
        // too little to go on
        assert_eq!(key_of([(60, 1.0)]), None);
    }

    #[test]
    fn needs_some_playing_first() {
        let mut k = KeyGuess::default();
        k.add(Some(0), 0.0);
        k.add(Some(0), 1.0);
        assert!(k.key.is_none());
    }
}
