//! The guitar tone: a small amp sim for a dry guitar signal (docs/PLAN.md §4.4, designed fresh). "As is" passes
//! the signal through untouched (for amp modellers like the MG-300).
//!
//! Chain: high-pass → noise gate → compressor → drive (pre-emphasis, 4x oversampled saturation) → tone stack
//! (bass, mid, treble, presence) → cabinet (speaker roll-off and resonance; skipped for bass and acoustic) →
//! chorus → delay → reverb → level. Changing a knob updates the running chain in place (coefficients and gains
//! glide), so tails keep ringing; changing the preset builds a new chain.

pub mod dsp;

use serde::{Deserialize, Serialize};

use crate::rt::ToneProcessor;
use dsp::{Biquad, Chorus, Compressor, Drive, Echo, Filter, Gate, Reverb, db_to_gain};

/// The knobs. Same names and ranges as the Python app's custom tone (its saved values carry over).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToneParams {
    /// Noise gate threshold, dB (-80 = off).
    pub gate: f32,
    /// Compression 0..10.
    pub comp: f32,
    /// Drive, dB 0..40.
    pub drive: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub presence: f32,
    /// Mix 0..1.
    pub chorus: f32,
    /// Mix 0..0.6.
    pub delay: f32,
    /// Seconds 0.05..0.8.
    pub delay_time: f32,
    /// 0..0.8.
    pub feedback: f32,
    /// Wet 0..0.6.
    pub reverb: f32,
    /// 0..1.
    pub room: f32,
    /// Output, dB -24..12.
    pub volume: f32,
    /// Speaker cabinet simulation.
    pub cab: bool,
}

impl Default for ToneParams {
    fn default() -> Self {
        ToneParams {
            gate: -56.0,
            comp: 3.0,
            drive: 10.0,
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            presence: 0.0,
            chorus: 0.0,
            delay: 0.0,
            delay_time: 0.32,
            feedback: 0.3,
            reverb: 0.12,
            room: 0.3,
            volume: 0.0,
            cab: true,
        }
    }
}

/// (key, label, low, high, default, unit) for every knob, for the settings page.
pub const KNOBS: [(&str, &str, f32, f32, f32, &str); 14] = [
    ("gate", "Noise gate", -80.0, -30.0, -56.0, "dB"),
    ("comp", "Compression", 0.0, 10.0, 3.0, ""),
    ("drive", "Drive", 0.0, 40.0, 10.0, "dB"),
    ("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    ("mid", "Mid", -12.0, 12.0, 0.0, "dB"),
    ("treble", "Treble", -12.0, 12.0, 0.0, "dB"),
    ("presence", "Presence", -12.0, 12.0, 0.0, "dB"),
    ("chorus", "Chorus", 0.0, 1.0, 0.0, "%"),
    ("delay", "Delay", 0.0, 0.6, 0.0, "%"),
    ("delay_time", "Delay time", 0.05, 0.8, 0.32, "s"),
    ("feedback", "Delay repeats", 0.0, 0.8, 0.3, "%"),
    ("reverb", "Reverb", 0.0, 0.6, 0.12, "%"),
    ("room", "Room size", 0.0, 1.0, 0.3, "%"),
    ("volume", "Volume", -24.0, 12.0, 0.0, "dB"),
];

impl ToneParams {
    pub fn get(&self, key: &str) -> Option<f32> {
        Some(match key {
            "gate" => self.gate,
            "comp" => self.comp,
            "drive" => self.drive,
            "bass" => self.bass,
            "mid" => self.mid,
            "treble" => self.treble,
            "presence" => self.presence,
            "chorus" => self.chorus,
            "delay" => self.delay,
            "delay_time" => self.delay_time,
            "feedback" => self.feedback,
            "reverb" => self.reverb,
            "room" => self.room,
            "volume" => self.volume,
            _ => return None,
        })
    }

    pub fn set(&mut self, key: &str, v: f32) {
        let (lo, hi) = KNOBS
            .iter()
            .find(|k| k.0 == key)
            .map(|k| (k.2, k.3))
            .unwrap_or((f32::MIN, f32::MAX));
        let v = v.clamp(lo, hi);
        match key {
            "gate" => self.gate = v,
            "comp" => self.comp = v,
            "drive" => self.drive = v,
            "bass" => self.bass = v,
            "mid" => self.mid = v,
            "treble" => self.treble = v,
            "presence" => self.presence = v,
            "chorus" => self.chorus = v,
            "delay" => self.delay = v,
            "delay_time" => self.delay_time = v,
            "feedback" => self.feedback = v,
            "reverb" => self.reverb = v,
            "room" => self.room = v,
            "volume" => self.volume = v,
            _ => {}
        }
    }

    /// The Python app's `tone_custom` dict.
    pub fn from_json(v: &serde_json::Value) -> ToneParams {
        let mut p = ToneParams::default();
        if let Some(m) = v.as_object() {
            for (k, x) in m {
                if let Some(f) = x.as_f64() {
                    p.set(k, f as f32);
                }
            }
        }
        p
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Clean,
    Crunch,
    Lead,
    Bass,
    Acoustic,
}

impl Preset {
    pub const ALL: [Preset; 5] = [
        Preset::Clean,
        Preset::Crunch,
        Preset::Lead,
        Preset::Bass,
        Preset::Acoustic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Clean => "Clean",
            Preset::Crunch => "Crunch",
            Preset::Lead => "Lead",
            Preset::Bass => "Bass",
            Preset::Acoustic => "Acoustic",
        }
    }

    pub fn params(self) -> ToneParams {
        let base = ToneParams {
            chorus: 0.0,
            delay: 0.0,
            ..ToneParams::default()
        };
        match self {
            Preset::Clean => ToneParams {
                gate: -58.0,
                comp: 4.0,
                drive: 2.0,
                treble: 1.0,
                reverb: 0.12,
                room: 0.25,
                volume: 0.0,
                ..base
            },
            Preset::Crunch => ToneParams {
                gate: -52.0,
                comp: 0.0,
                drive: 24.0,
                mid: 3.0,
                treble: -1.0,
                reverb: 0.1,
                room: 0.2,
                ..base
            },
            Preset::Lead => ToneParams {
                gate: -52.0,
                comp: 5.0,
                drive: 34.0,
                mid: 4.0,
                delay: 0.16,
                delay_time: 0.32,
                feedback: 0.25,
                reverb: 0.14,
                room: 0.35,
                ..base
            },
            Preset::Bass => ToneParams {
                gate: -60.0,
                comp: 5.0,
                drive: 0.0,
                bass: 3.0,
                mid: -1.0,
                treble: -2.0,
                presence: -3.0,
                reverb: 0.0,
                cab: false,
                ..base
            },
            Preset::Acoustic => ToneParams {
                gate: -80.0,
                comp: 3.0,
                drive: 0.0,
                bass: -3.0,
                presence: 2.0,
                reverb: 0.15,
                room: 0.3,
                cab: false,
                ..base
            },
        }
    }
}

/// Which tone is on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tone", rename_all = "lowercase")]
pub enum ToneSpec {
    /// No processing: the input as it is (for amp modellers).
    #[default]
    AsIs,
    Preset {
        preset: Preset,
    },
    Custom {
        params: ToneParams,
    },
}

impl ToneSpec {
    pub fn params(&self) -> Option<ToneParams> {
        match self {
            ToneSpec::AsIs => None,
            ToneSpec::Preset { preset } => Some(preset.params()),
            ToneSpec::Custom { params } => Some(*params),
        }
    }

    /// The Python app's tone names ("as is", "clean", "crunch", "lead", "custom").
    pub fn from_python(name: &str, custom: Option<&serde_json::Value>) -> ToneSpec {
        match name {
            "clean" => ToneSpec::Preset {
                preset: Preset::Clean,
            },
            "crunch" => ToneSpec::Preset {
                preset: Preset::Crunch,
            },
            "lead" => ToneSpec::Preset { preset: Preset::Lead },
            "custom" => ToneSpec::Custom {
                params: custom.map(ToneParams::from_json).unwrap_or_default(),
            },
            _ => ToneSpec::AsIs,
        }
    }
}

/// The running amp sim.
pub struct ToneChain {
    rate: f32,
    p: ToneParams,
    hpf: Biquad,
    gate: Gate,
    comp: Compressor,
    comp_on: bool,
    pre: Biquad,
    drive: Drive,
    drive_on: bool,
    drive_comp: f32,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    presence: Biquad,
    cab: [Biquad; 4],
    chorus: Chorus,
    echo: Echo,
    reverb: Reverb,
    level: f32,
    level_target: f32,
}

impl ToneChain {
    pub fn new(rate: u32, p: ToneParams) -> ToneChain {
        let r = rate as f32;
        let mut c = ToneChain {
            rate: r,
            p,
            hpf: Biquad::new(r, Filter::HighPass { f: 80.0, q: 0.707 }),
            gate: Gate::new(r),
            comp: Compressor::new(r),
            comp_on: false,
            pre: Biquad::new(
                r,
                Filter::Peak {
                    f: 720.0,
                    q: 0.8,
                    db: 0.0,
                },
            ),
            drive: Drive::new(r),
            drive_on: false,
            drive_comp: 1.0,
            bass: Biquad::new(r, Filter::LowShelf { f: 120.0, db: 0.0 }),
            mid: Biquad::new(
                r,
                Filter::Peak {
                    f: 800.0,
                    q: 0.8,
                    db: 0.0,
                },
            ),
            treble: Biquad::new(r, Filter::HighShelf { f: 3200.0, db: 0.0 }),
            presence: Biquad::new(
                r,
                Filter::Peak {
                    f: 5000.0,
                    q: 1.0,
                    db: 0.0,
                },
            ),
            cab: [
                Biquad::new(r, Filter::HighPass { f: 90.0, q: 0.7 }),
                Biquad::new(
                    r,
                    Filter::Peak {
                        f: 2400.0,
                        q: 1.4,
                        db: 3.0,
                    },
                ),
                Biquad::new(r, Filter::LowPass { f: 5400.0, q: 0.8 }),
                Biquad::new(r, Filter::LowPass { f: 6200.0, q: 0.6 }),
            ],
            chorus: Chorus::new(r),
            echo: Echo::new(r),
            reverb: Reverb::new(r),
            level: 1.0,
            level_target: 1.0,
        };
        c.update(p);
        c.level = c.level_target;
        c
    }

    /// New knob values on the running chain.
    pub fn update(&mut self, p: ToneParams) {
        let r = self.rate;
        self.p = p;
        self.hpf.set(
            r,
            Filter::HighPass {
                f: if p.cab { 80.0 } else { 35.0 },
                q: 0.707,
            },
        );
        self.gate.set_threshold_db(p.gate);
        self.comp_on = p.comp > 0.05;
        self.comp.threshold_db = -14.0 - 2.0 * p.comp;
        self.comp.ratio = 1.0 + p.comp * 0.7;
        self.comp.makeup = db_to_gain(p.comp * 0.9);
        self.drive_on = p.drive > 0.5;
        self.pre.set(
            r,
            Filter::Peak {
                f: 720.0,
                q: 0.8,
                db: (p.drive / 40.0) * 6.0,
            },
        );
        self.drive.gain = db_to_gain(p.drive);
        // keep the level roughly constant as the drive goes up
        self.drive_comp = db_to_gain(-p.drive * 0.45);
        self.bass.set(r, Filter::LowShelf { f: 120.0, db: p.bass });
        self.mid.set(
            r,
            Filter::Peak {
                f: 800.0,
                q: 0.8,
                db: p.mid,
            },
        );
        self.treble.set(
            r,
            Filter::HighShelf {
                f: 3200.0,
                db: p.treble,
            },
        );
        self.presence.set(
            r,
            Filter::Peak {
                f: 5000.0,
                q: 1.0,
                db: p.presence,
            },
        );
        self.chorus.mix = p.chorus;
        self.echo.mix = p.delay;
        self.echo.time_s = p.delay_time;
        self.echo.feedback = p.feedback;
        self.reverb.wet = p.reverb;
        self.reverb.room = p.room;
        self.reverb.dry = 1.0 - p.reverb * 0.4;
        self.level_target = db_to_gain(p.volume);
    }

    pub fn params(&self) -> ToneParams {
        self.p
    }
}

impl ToneProcessor for ToneChain {
    fn process(&mut self, x: &mut [f32]) {
        let n = x.len().max(1) as f32;
        let step = (self.level_target - self.level) / n;
        for s in x.iter_mut() {
            let mut v = self.hpf.tick(*s);
            v = self.gate.tick(v);
            if self.comp_on {
                v = self.comp.tick(v);
            }
            if self.drive_on {
                v = self.drive.tick(self.pre.tick(v)) * self.drive_comp;
            }
            v = self
                .presence
                .tick(self.treble.tick(self.mid.tick(self.bass.tick(v))));
            if self.p.cab {
                for b in &mut self.cab {
                    v = b.tick(v);
                }
            }
            if self.p.chorus > 0.005 {
                v = self.chorus.tick(v);
            }
            if self.p.delay > 0.005 {
                v = self.echo.tick(v);
            }
            v = self.reverb.tick(v);
            self.level += step;
            *s = v * self.level;
        }
        self.level = self.level_target;
    }

    fn reset(&mut self) {
        *self = ToneChain::new(self.rate as u32, self.p);
    }

    fn set_params(&mut self, p: &ToneParams) {
        self.update(*p);
    }
}

/// The processor for a tone at a sample rate (None for "as is"). Builds delay lines: call off the audio thread.
pub fn make_tone(spec: &ToneSpec, rate: u32) -> Option<Box<dyn ToneProcessor>> {
    spec.params()
        .map(|p| Box::new(ToneChain::new(rate, p)) as Box<dyn ToneProcessor>)
}

#[cfg(test)]
mod tests {
    use super::dsp::{rms, sine};
    use super::*;

    fn run(spec: ToneSpec, x: &[f32]) -> Vec<f32> {
        let mut t = make_tone(&spec, 48000).unwrap();
        let mut y = x.to_vec();
        for chunk in y.chunks_mut(128) {
            t.process(chunk);
        }
        y
    }

    #[test]
    fn every_preset_is_finite_bounded_and_keeps_a_sensible_level() {
        let x = sine(48000.0, 196.0, 48000, 0.3);
        for p in Preset::ALL {
            let y = run(ToneSpec::Preset { preset: p }, &x);
            assert!(y.iter().all(|v| v.is_finite()), "{p:?}");
            let level = rms(&y[24000..]);
            assert!((0.03..1.5).contains(&level), "{p:?}: rms {level}");
        }
    }

    #[test]
    fn silence_stays_silent() {
        let x = vec![0.0f32; 48000];
        for p in Preset::ALL {
            let y = run(ToneSpec::Preset { preset: p }, &x);
            assert!(rms(&y) < 1e-4, "{p:?}");
        }
    }

    #[test]
    fn knob_changes_apply_in_place() {
        let mut t = ToneChain::new(48000, Preset::Clean.params());
        let mut p = t.params();
        p.volume = -12.0;
        t.update(p);
        let mut x = sine(48000.0, 440.0, 9600, 0.2);
        t.process(&mut x);
        assert_eq!(t.params().volume, -12.0);
    }

    #[test]
    fn python_names_and_custom_values_carry_over() {
        let custom =
            serde_json::json!({"gate": -56.0, "drive": 10.0, "reverb": 0.12, "volume": 3.0, "bogus": 1});
        let s = ToneSpec::from_python("custom", Some(&custom));
        let ToneSpec::Custom { params } = s else { panic!() };
        assert_eq!(params.volume, 3.0);
        assert_eq!(ToneSpec::from_python("as is", None), ToneSpec::AsIs);
        assert_eq!(
            ToneSpec::from_python("lead", None),
            ToneSpec::Preset { preset: Preset::Lead }
        );
        let j = serde_json::to_string(&ToneSpec::Preset {
            preset: Preset::Crunch,
        })
        .unwrap();
        assert_eq!(j, r#"{"tone":"preset","preset":"crunch"}"#);
    }

    #[test]
    fn cost_per_block_is_small() {
        let mut t = make_tone(&ToneSpec::Preset { preset: Preset::Lead }, 48000).unwrap();
        let mut x = sine(48000.0, 196.0, 128, 0.3);
        let start = std::time::Instant::now();
        for _ in 0..375 {
            // 1 s of audio in 128-frame blocks
            t.process(&mut x);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        // debug builds are slow; release is ~10x faster. Keep well under real time even in debug.
        assert!(ms < 900.0, "{ms} ms for 1 s of audio");
    }
}
