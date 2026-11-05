//! What the guitar input is doing, for the home screen's AMP ROOM button and the amp room's strip (PLAN.md P22.1,
//! P22.2): its level on a VU needle with real ballistics, the peak lamp, and the note played (the tuner's pitch
//! reading, as the player's). New code (no Python source).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use qrs_analysis::spectrum::Spectra;
use qrs_audio::Engine;

/// The VU's integration time: a real VU meter reaches 99% of a step in 300 ms (a time constant of about 65 ms).
const VU_TAU: f32 = 0.065;
/// The needle's scale: -42 dBFS (RMS) at rest, -6 at full scale; the last eighth is the red zone.
const VU_LO: f32 = -42.0;
const VU_HI: f32 = -6.0;
/// Below this RMS the input counts as silent (no repaints for it).
const GATE: f32 = 0.0015;

pub struct Listen {
    sp: Spectra,
    buf: Vec<f32>,
    hist: VecDeque<f64>,
    /// The note heard (MIDI, fractional), while one is.
    pub midi: Option<f64>,
    seen: Instant,
    pitch_at: Option<Instant>,
    /// The needle, 0..1 (moving as a VU's does).
    pub needle: f32,
    /// The level (RMS) and peak now, dBFS.
    pub level_db: f32,
    pub peak_db: f32,
    /// The highest peak in the last second or so (dBFS), for the strip's peak hold.
    pub hold_db: f32,
    hold_at: Option<Instant>,
    clips_seen: Option<u32>,
    /// The peak lamp's on until then (a clip, or a peak at -1 dBFS or more).
    lamp_until: Option<Instant>,
    last: Option<Instant>,
    /// Screenshots: a level and note to show instead of the input's.
    fake: Option<(f32, f64)>,
    /// What the input plays (P32.5): a bass's notes are read lower, over a longer window.
    instrument: qrs_analysis::instrument::Instrument,
}

impl Default for Listen {
    fn default() -> Self {
        Listen {
            sp: Spectra::new(),
            buf: vec![0.0; 4096],
            hist: VecDeque::new(),
            midi: None,
            seen: Instant::now(),
            pitch_at: None,
            needle: 0.0,
            level_db: -120.0,
            peak_db: -120.0,
            hold_db: -120.0,
            hold_at: None,
            clips_seen: None,
            lamp_until: None,
            last: None,
            fake: None,
            instrument: qrs_analysis::instrument::Instrument::Guitar,
        }
    }
}

fn db(v: f32) -> f32 {
    20.0 * v.max(1e-6).log10()
}

/// Where the needle points for a level (dBFS RMS): 0..1.
pub fn vu_pos(level_db: f32) -> f32 {
    ((level_db - VU_LO) / (VU_HI - VU_LO)).clamp(0.0, 1.0)
}

impl Listen {
    /// Screenshot mode: show this RMS level (dBFS) and note (MIDI) with the needle settled on it (and the peak
    /// lamp lit, `hot`).
    pub fn stage(&mut self, level_db: f32, midi: f64, hot: bool) {
        self.fake = Some((level_db, midi));
        self.lamp_until = hot.then(|| Instant::now() + Duration::from_secs(3600));
        self.level_db = level_db;
        self.peak_db = if hot { -0.4 } else { level_db + 9.0 };
        self.hold_db = level_db + 11.0;
        self.needle = vu_pos(level_db);
        self.midi = Some(midi);
    }

    /// The instrument the input plays (a guitar unless set).
    pub fn set_instrument(&mut self, inst: qrs_analysis::instrument::Instrument) {
        if inst != self.instrument {
            self.instrument = inst;
            self.buf = vec![0.0; inst.pitch_window(48000)];
            self.hist.clear();
        }
    }

    /// Read the input (once a frame it's drawn). Returns whether it's live: sound coming in or the needle still
    /// moving (the caller repaints soon); otherwise it's still and only needs a slow look for sound starting.
    pub fn update(&mut self, e: Option<&Engine>) -> bool {
        let now = Instant::now();
        let dt = self
            .last
            .map(|t| (now - t).as_secs_f32().min(0.25))
            .unwrap_or(0.0);
        self.last = Some(now);
        if let Some((lvl, midi)) = self.fake {
            self.level_db = lvl;
            self.midi = Some(midi);
            return false;
        }
        let Some(e) = e.filter(|e| e.status().input.is_some()) else {
            self.level_db = -120.0;
            self.peak_db = -120.0;
            self.midi = None;
            let moving = self.settle(0.0, dt);
            return moving;
        };
        let m = e.meters();
        self.level_db = db(m.level);
        self.peak_db = db(m.peak);
        if self.peak_db >= self.hold_db || self.hold_at.is_none_or(|t| now - t > Duration::from_millis(1200))
        {
            self.hold_db = self.peak_db;
            self.hold_at = Some(now);
        }
        if self.clips_seen.is_some_and(|c| m.clips > c) || self.peak_db > -1.0 {
            self.lamp_until = Some(now + Duration::from_millis(700));
        }
        self.clips_seen = Some(m.clips);
        let sounding = m.level > GATE;
        // the note: the tuner's reading (at most 20 times a second, and only with sound)
        if sounding && self.pitch_at.is_none_or(|t| now - t > Duration::from_millis(50)) {
            self.pitch_at = Some(now);
            let rate = e.capture().rate();
            if e.capture().latest(&mut self.buf).is_some() {
                let (hz, clarity) =
                    qrs_analysis::instrument::pitch_for(self.instrument, &mut self.sp, &self.buf, rate);
                if hz > 0.0 && clarity > 0.85 {
                    self.hist.push_back(69.0 + 12.0 * (hz / 440.0).log2());
                    while self.hist.len() > 5 {
                        self.hist.pop_front();
                    }
                    let mut v: Vec<f64> = self.hist.iter().copied().collect();
                    v.sort_by(f64::total_cmp);
                    self.midi = Some(v[v.len() / 2]);
                    self.seen = now;
                }
            }
        }
        if now - self.seen > Duration::from_millis(1500) {
            self.midi = None;
            self.hist.clear();
        }
        let moving = self.settle(vu_pos(self.level_db), dt);
        sounding || moving || self.lamp()
    }

    /// The needle toward `target` as a VU's (true while it's still moving).
    fn settle(&mut self, target: f32, dt: f32) -> bool {
        let a = 1.0 - (-dt / VU_TAU).exp();
        self.needle += (target - self.needle) * a;
        if (target - self.needle).abs() < 0.002 {
            self.needle = target;
            return false;
        }
        true
    }

    /// The peak lamp is lit.
    pub fn lamp(&self) -> bool {
        self.lamp_until.is_some_and(|t| Instant::now() < t)
    }

    /// Too loud: the lamp lit by a clip or a peak near full scale.
    pub fn hot(&self) -> bool {
        self.lamp()
    }

    /// The note heard: (name, octave, cents), e.g. ("E", 2, 3).
    pub fn note(&self) -> Option<(String, i32, i32)> {
        let m = self.midi?;
        let r = m.round() as i32;
        Some((
            qrs_formats::model::note_name(r).to_string(),
            r.div_euclid(12) - 1,
            ((m - m.round()) * 100.0).round() as i32,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_needle_moves_as_a_vu() {
        let mut l = Listen::default();
        // a step to full scale: about 99% in 300 ms, frame by frame
        let mut t = 0.0;
        while t < 0.3 {
            l.settle(1.0, 1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        assert!(l.needle > 0.98 && l.needle <= 1.0, "{}", l.needle);
        // and it settles (stops asking for frames)
        for _ in 0..200 {
            l.settle(1.0, 1.0 / 60.0);
        }
        assert!(!l.settle(1.0, 1.0 / 60.0));
        assert_eq!(vu_pos(-120.0), 0.0);
        assert_eq!(vu_pos(0.0), 1.0);
        assert!((vu_pos(-24.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn no_input_is_still() {
        let mut l = Listen::default();
        assert!(!l.update(None));
        assert!(l.note().is_none());
        l.stage(-20.0, 40.03, false);
        assert_eq!(l.note(), Some(("E".to_string(), 2, 3)));
    }
}
