//! Speaker cabinets as filters: the speaker's resonance in its box (a bigger bump for a closed back), the low-mid
//! dip, the upper-mid presence peak and cone break-up, and the steep roll-off above 4-5 kHz that makes an amp
//! sound like an amp rather than a fizzy preamp; then the microphone: its place on the cone (edge: darker, cap:
//! brighter), its distance (proximity bass when close, a floor reflection and less air when further), its type
//! (dynamic: a presence lift; ribbon: dark and smooth; condenser: open top), and low and high cuts.
//!
//! Bass cabs (P37.2): the 8x10, a 4x10 (tight, a brighter break-up), a 2x10 (less low end), a 4x15 (deep and dark)
//! and the 1x15; each has a Horn (the tweeter of an Eden, Hartke or Ampeg HLF cab: a band from ~3.5 to 9 kHz taken
//! off the amp's output and added) and a DI knob: the pre-cab signal (a gentle 9 kHz low-pass) blended with the mic,
//! as bass is usually recorded. Both at 0 leave the cab exactly as it was.
//!
//! Mic model (P38.2, D156; the last knob): named voicings in place of the Mic type's generic one, from the published
//! responses of the mics studios use on cabs: a 57-style dynamic (lean lows, a sharp 5-6 kHz presence peak, a fall
//! above 9 kHz), a 121-style ribbon (full, smooth, rolled off from 7 kHz), a 421-style dynamic (fuller than the 57, a
//! broad 4 kHz lift) and an 87-style large-diaphragm condenser (even, a gentle air lift). "As type" is as before.

use super::dsp::{Biquad, Filter, Line, unit10};
use super::{MAX_KNOBS, Pedal};

#[derive(Clone, Copy, Debug)]
struct Voice {
    /// The box's low cut (high-pass), the speaker's resonance and how much it bumps.
    hp: f32,
    res: f32,
    res_db: f32,
    /// The low-mid dip.
    lm: f32,
    lm_db: f32,
    /// Presence peak and cone break-up.
    pres: f32,
    pres_db: f32,
    bk: f32,
    bk_db: f32,
    /// Where the top falls away.
    lp: f32,
    trim_db: f32,
    /// A bass cab (its Horn and DI knobs come before the Mic model).
    bass: bool,
}

const C1X10: Voice = Voice {
    hp: 90.0,
    res: 110.0,
    res_db: 2.0,
    lm: 650.0,
    lm_db: -2.0,
    pres: 2200.0,
    pres_db: 4.0,
    bk: 3600.0,
    bk_db: -3.0,
    lp: 5200.0,
    trim_db: 1.0,
    bass: false,
};
const C1X12: Voice = Voice {
    hp: 80.0,
    res: 100.0,
    res_db: 2.5,
    lm: 500.0,
    lm_db: -2.0,
    pres: 2500.0,
    pres_db: 4.0,
    bk: 3800.0,
    bk_db: -3.0,
    lp: 5000.0,
    trim_db: 1.0,
    bass: false,
};
const C2X12: Voice = Voice {
    hp: 75.0,
    res: 95.0,
    res_db: 3.0,
    lm: 450.0,
    lm_db: -2.5,
    pres: 2700.0,
    pres_db: 4.5,
    bk: 4000.0,
    bk_db: -4.0,
    lp: 5300.0,
    trim_db: 0.5,
    bass: false,
};
const C4X12: Voice = Voice {
    hp: 75.0,
    res: 90.0,
    res_db: 4.0,
    lm: 400.0,
    lm_db: -3.0,
    pres: 2900.0,
    pres_db: 5.0,
    bk: 4300.0,
    bk_db: -5.0,
    lp: 4800.0,
    trim_db: 0.0,
    bass: false,
};
const C8X10: Voice = Voice {
    hp: 45.0,
    res: 70.0,
    res_db: 4.0,
    lm: 300.0,
    lm_db: -2.0,
    pres: 1800.0,
    pres_db: 3.0,
    bk: 3000.0,
    bk_db: -4.0,
    lp: 4000.0,
    trim_db: 0.0,
    bass: true,
};
const C1X15: Voice = Voice {
    hp: 40.0,
    res: 60.0,
    res_db: 4.0,
    lm: 400.0,
    lm_db: -1.0,
    pres: 1200.0,
    pres_db: 2.0,
    bk: 2200.0,
    bk_db: -3.0,
    lp: 2800.0,
    trim_db: 1.0,
    bass: true,
};

const C4X10: Voice = Voice {
    hp: 50.0,
    res: 80.0,
    res_db: 3.0,
    lm: 350.0,
    lm_db: -2.0,
    pres: 2500.0,
    pres_db: 4.0,
    bk: 3800.0,
    bk_db: -3.0,
    lp: 5000.0,
    trim_db: 0.0,
    bass: true,
};
const C2X10: Voice = Voice {
    hp: 60.0,
    res: 90.0,
    res_db: 2.5,
    lm: 400.0,
    lm_db: -1.5,
    pres: 2300.0,
    pres_db: 3.5,
    bk: 3600.0,
    bk_db: -3.0,
    lp: 4800.0,
    trim_db: 1.0,
    bass: true,
};
const C4X15: Voice = Voice {
    hp: 35.0,
    res: 55.0,
    res_db: 5.0,
    lm: 350.0,
    lm_db: -2.0,
    pres: 1100.0,
    pres_db: 1.5,
    bk: 2000.0,
    bk_db: -3.0,
    lp: 2500.0,
    trim_db: 1.0,
    bass: true,
};

/// P41.2 (D167): a guitar 4x10 (the tweed Bassman's, the Super Reverb's): four open-backed tens, a small, high
/// resonance, little low-mid dip, an earlier and brighter presence peak than the twelves, a soft top.
const C4X10_OPEN: Voice = Voice {
    hp: 70.0,
    res: 115.0,
    res_db: 2.0,
    lm: 550.0,
    lm_db: -1.5,
    pres: 2000.0,
    pres_db: 4.5,
    bk: 3400.0,
    bk_db: -3.0,
    lp: 5600.0,
    trim_db: 0.0,
    bass: false,
};

struct Cab {
    voice: Voice,
    rate: f32,
    f: [Biquad; 12],
    /// The horn's band (a high-pass and a low-pass on the cab's input) and its level; the DI's low-pass and share.
    horn_f: [Biquad; 2],
    horn: f32,
    di_lp: Biquad,
    di: f32,
    /// The floor reflection: a short delay mixed in as the mic moves away.
    refl: Line,
    refl_d: usize,
    refl_g: f32,
    trim: f32,
}

fn new(voice: Voice, rate: f32) -> Box<dyn Pedal> {
    Box::new(Cab {
        voice,
        rate,
        f: [Biquad::default(); 12],
        refl: Line::new((rate * 0.004) as usize + 4),
        refl_d: 1,
        refl_g: 0.0,
        trim: 1.0,
        horn_f: [
            Biquad::new(rate, Filter::HighPass { f: 3500.0, q: 0.7 }),
            Biquad::new(
                rate,
                Filter::LowPass {
                    f: (9000.0f32).min(rate * 0.45),
                    q: 0.7,
                },
            ),
        ],
        horn: 0.0,
        di_lp: Biquad::new(
            rate,
            Filter::LowPass {
                f: (9000.0f32).min(rate * 0.45),
                q: 0.7,
            },
        ),
        di: 0.0,
    })
}

/// The cab's response in dB at `f` (for the big view's picture).
pub fn response_db(id: &str, v: &[f32; MAX_KNOBS], rate: f32, f: f32) -> f32 {
    let voice = match id {
        "cab_1x10" => C1X10,
        "cab_1x12" => C1X12,
        "cab_2x12" => C2X12,
        "cab_4x12" => C4X12,
        "cab_8x10" => C8X10,
        "cab_4x10" => C4X10,
        "cab_2x10" => C2X10,
        "cab_4x15" => C4X15,
        "cab_4x10_open" => C4X10_OPEN,
        _ => C1X15,
    };
    let mut g = 1.0f32;
    for flt in filters(voice, v, rate) {
        g *= Biquad::new(rate, flt).magnitude(f, rate);
    }
    20.0 * g.max(1e-6).log10() + voice.trim_db
}

pub fn c1x10(rate: f32) -> Box<dyn Pedal> {
    new(C1X10, rate)
}
pub fn c1x12(rate: f32) -> Box<dyn Pedal> {
    new(C1X12, rate)
}
pub fn c2x12(rate: f32) -> Box<dyn Pedal> {
    new(C2X12, rate)
}
pub fn c4x12(rate: f32) -> Box<dyn Pedal> {
    new(C4X12, rate)
}
pub fn c8x10(rate: f32) -> Box<dyn Pedal> {
    new(C8X10, rate)
}
pub fn c1x15(rate: f32) -> Box<dyn Pedal> {
    new(C1X15, rate)
}
pub fn c4x10(rate: f32) -> Box<dyn Pedal> {
    new(C4X10, rate)
}
pub fn c2x10(rate: f32) -> Box<dyn Pedal> {
    new(C2X10, rate)
}
pub fn c4x15(rate: f32) -> Box<dyn Pedal> {
    new(C4X15, rate)
}
pub fn c4x10_open(rate: f32) -> Box<dyn Pedal> {
    new(C4X10_OPEN, rate)
}

/// The Mic model knob's position (its index differs: a bass cab has Horn and DI before it).
fn mic_model(c: Voice, v: &[f32; MAX_KNOBS]) -> i32 {
    v[if c.bass { 7 } else { 5 }].round() as i32
}

/// The speaker, box and mic as filters, for these knobs.
fn filters(c: Voice, v: &[f32; MAX_KNOBS], r: f32) -> [Filter; 12] {
    let (pos, dist, mic, low_cut, high_cut) = (unit10(v[0]), unit10(v[1]), v[2].round() as i32, v[3], v[4]);
    // the mic's type: (low shelf dB, presence dB, presence Hz, its Q, air shelf dB, air Hz)
    let (mic_low, mic_pres, pres_f, pres_q, mic_air, air_f) = match (mic_model(c, v), mic) {
        (1, _) => (-2.5, 5.0, 5800.0, 1.5, -3.0, 9500.0), // 57-style: lean lows, a sharp presence peak
        (2, _) => (2.5, -1.5, 4000.0, 0.8, -7.0, 7000.0), // 121-style ribbon: full, smooth, dark top
        (3, _) => (0.5, 3.5, 4200.0, 0.9, -1.5, 9000.0),  // 421-style: fuller lows, a broad upper-mid lift
        (4, _) => (0.5, 1.5, 3000.0, 0.7, 2.5, 9000.0),   // 87-style condenser: even, a little air
        (_, 1) => (2.0, -1.0, 5000.0, 1.2, -5.0, 6000.0), // ribbon: warm, dark, smooth
        (_, 2) => (1.0, 0.5, 5000.0, 1.2, 3.0, 6000.0),   // condenser: flat, open top
        _ => (-1.5, 3.5, 5000.0, 1.2, 0.0, 6000.0), // dynamic: the SM57's presence lift and lighter lows
    };
    [
        Filter::HighPass { f: c.hp, q: 0.65 },
        Filter::Peak {
            f: c.res,
            q: 1.2,
            db: c.res_db,
        },
        Filter::Peak {
            f: c.lm,
            q: 0.9,
            db: c.lm_db,
        },
        // the cap of the cone is brighter than its edge
        Filter::Peak {
            f: c.pres,
            q: 1.0,
            db: c.pres_db + 4.0 * pos - 1.0,
        },
        Filter::Peak {
            f: c.bk,
            q: 3.0,
            db: c.bk_db,
        },
        Filter::LowPass {
            f: c.lp * (0.78 + 0.5 * pos),
            q: 0.85,
        },
        Filter::LowPass {
            f: c.lp * (0.95 + 0.6 * pos),
            q: 0.6,
        },
        // proximity effect up close, the mic's own colour
        Filter::LowShelf {
            f: 160.0,
            db: 5.0 * (1.0 - dist) - 1.0 + mic_low,
        },
        Filter::Peak {
            f: pres_f,
            q: pres_q,
            db: mic_pres,
        },
        Filter::HighShelf {
            f: air_f,
            db: mic_air - 3.0 * dist,
        },
        Filter::HighPass {
            f: low_cut.clamp(15.0, 400.0),
            q: 0.7,
        },
        Filter::LowPass {
            f: high_cut.clamp(1500.0, r * 0.45),
            q: 0.7,
        },
    ]
}

impl Pedal for Cab {
    fn process(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            let mut s = *v;
            for b in &mut self.f {
                s = b.tick(s);
            }
            self.refl.push(s);
            let x0 = *v;
            let mut y = (s + self.refl.at(self.refl_d) * self.refl_g) * self.trim;
            if self.horn > 0.0 {
                let h = self.horn_f[0].tick(x0);
                y += self.horn_f[1].tick(h) * self.horn;
            }
            if self.di > 0.0 {
                // the mic and the DI are only partly alike: a plain mix dipped 2.5 dB at half and half
                let k = 1.0 + 0.33 * 4.0 * self.di * (1.0 - self.di);
                y = (y * (1.0 - self.di) + self.di_lp.tick(x0) * self.di) * k;
            }
            *v = y;
        }
    }

    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        let (r, dist) = (self.rate, unit10(v[1]));
        let fs = filters(self.voice, v, r);
        for (b, f) in self.f.iter_mut().zip(fs) {
            b.set(r, f);
        }
        // the floor bounce: 0.3 ms close up to 2.6 ms at arm's length, getting louder with distance
        self.refl_d = ((0.3 + 2.3 * dist) * 1e-3 * r).round().max(1.0) as usize;
        self.refl_g = 0.45 * dist;
        self.trim = 10f32.powf((self.voice.trim_db - 2.0 * dist) / 20.0) / (1.0 + self.refl_g * 0.5);
        // a bass cab's horn (0..10: up to about the cone's level in its band) and DI share (guitar cabs: none)
        if self.voice.bass {
            self.horn = unit10(v[5]);
            self.di = (v[6] / 100.0).clamp(0.0, 1.0);
        }
    }

    fn reset(&mut self) {
        for b in &mut self.f {
            b.reset();
        }
        self.refl.clear();
        for b in &mut self.horn_f {
            b.reset();
        }
        self.di_lp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{MODELS, catalog};
    use super::*;

    #[test]
    fn each_mic_model_has_its_own_voice_and_as_type_is_as_before() {
        for id in ["cab_4x12", "cab_8x10"] {
            let m = &MODELS[catalog::find(id).unwrap()];
            let k = m.knob_index("mic_model").expect("mic model");
            assert_eq!(k, m.knobs.len() - 1, "the last knob");
            let base = m.defaults();
            assert_eq!(base[k], 0.0, "As type by default");
            let at = |v: &[f32; MAX_KNOBS], f: f32| response_db(id, v, 48000.0, f);
            let mut curves = Vec::new();
            for mm in 1..=4 {
                let mut v = base;
                v[k] = mm as f32;
                let d: Vec<f32> = [120.0, 1000.0, 4500.0, 6000.0, 9000.0]
                    .iter()
                    .map(|f| at(&v, *f) - at(&base, *f))
                    .collect();
                // a voicing, not a different cab: the mids within 1 dB, the edges moved
                assert!(d[1].abs() < 1.0, "{id} {mm}: {d:?}");
                assert!(d.iter().any(|x| x.abs() > 1.0), "{id} {mm}: {d:?}");
                curves.push(d);
            }
            for i in 0..curves.len() {
                for j in i + 1..curves.len() {
                    let diff = curves[i]
                        .iter()
                        .zip(&curves[j])
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0, f32::max);
                    assert!(diff > 1.0, "{id}: mic models {} and {} alike", i + 1, j + 1);
                }
            }
        }
    }
}
