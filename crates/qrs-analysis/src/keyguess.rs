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
        let dt = self.last.map(|l| (now - l).max(0.0)).unwrap_or(0.0);
        self.last = Some(now);
        let fade = 0.5f64.powf(dt / FADE_HALF_LIFE);
        for h in &mut self.hist {
            *h *= fade;
        }
        if let Some(pc) = pc {
            self.hist[(pc % 12) as usize] += dt;
        }
        if self.hist.iter().sum::<f64>() < MIN_WEIGHT {
            return;
        }
        let mut best: Option<(Key, f64)> = None;
        let mut current_score = None;
        for mode in [Mode::Major, Mode::Minor] {
            for t in 0..12u8 {
                let s = corr(&self.hist, &profile(t as usize, mode));
                if best.is_none_or(|(_, b)| s > b) {
                    best = Some(((t, mode), s));
                }
                if self.key == Some((t, mode)) {
                    current_score = Some(s);
                }
            }
        }
        let (best_key, best_score) = best.unwrap();
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
        self.key.map(|(t, m)| {
            format!(
                "{} {}",
                qrs_formats::model::note_name(t as i32),
                if m == Mode::Major { "major" } else { "minor" }
            )
        })
    }
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
    fn needs_some_playing_first() {
        let mut k = KeyGuess::default();
        k.add(Some(0), 0.0);
        k.add(Some(0), 1.0);
        assert!(k.key.is_none());
    }
}
