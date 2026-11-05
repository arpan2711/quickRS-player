//! Overdrive, distortion and fuzz. Each is a "simplified, physically-informed" model in Yeh, Abel and Smith's
//! sense (DAFx-07): a conditioning filter, gain, a memoryless clipper and an equalising filter, with the filters and
//! gains taken from the circuit (the Tube Screamer's 720 Hz gain-path high-pass and its clean path summed back in,
//! the RAT's two RC branches and the LM308's shrinking bandwidth, the DS-1's and Big Muff's mid-scooped tone
//! stacks, the Fuzz Face's lopsided bias that shifts as you dig in). The clippers run at twice the rate with
//! first-order antiderivative antialiasing (`dsp::Shaper`).
//!
//! The overdrives with a diode pair to ground or in the feedback loop (Mean Green, Delta Blue, Gold Pegasus, Copper
//! Top, Tangerine Dist, Rodent) have a `diodes` choice inside the box (D45): the pedal's own pair by default, or
//! silicon, asymmetric, LED or germanium, as a modder would swap them.
//!
//! Every drive and fuzz has a Blend and a "Clean low" (P37.2, for bass): how much of the effect, the rest the dry
//! input, and a crossover under which only the dry plays (`dsp::Blend`). At 100% and Off the path is exactly the old
//! one. The bass voices: the B7K-style grinder (Grunt and Attack shelves before the clipper, a 4-band EQ after) and
//! the Bass Big Muff (the Muff with its lows kept and a Bass Boost).
//!
//! Levels: the input is a guitar at about -12 dBFS peaks; a diode's knee sits near 0.37 of full scale.

use super::dsp::{
    Biquad, Blend, Curve, DcBlock, Filter, OnePole, Oversample2, Shaper, Smooth, audio_taper, db_to_gain,
    sweep, unit10,
};
use super::{MAX_KNOBS, Pedal};

const DIODE: f32 = 0.37;

/// A diode pair swapped in (`catalog::DIODES`): its curve and knee. Silicon: a symmetric soft knee at 0.6 V;
/// asymmetric: one diode one way, two the other (even harmonics); LED: about 1.7 V, so clipping comes later, louder
/// and with a sharper corner; germanium: 0.3 V, a long soft knee.
fn diode(i: usize) -> (Curve, f32) {
    match i {
        0 => (Curve::Tanh, DIODE),
        1 => (Curve::Asym(0.6), DIODE),
        2 => (Curve::Hard, 0.95),
        _ => (Curve::Soft, 0.25),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Voice {
    Green,
    Blues,
    Gold,
    Germanium,
    Treble,
    Orange,
    Rodent,
    Metal,
    BassDrive,
    RoundFuzz,
    BigPi,
    Bender,
    OctaveFuzz,
    BassGrind,
    BassMuff,
}

/// How much louder the dry input is made for the Blend, dB: the effect path runs this much over its input before the
/// Level knob (measured at each voice's defaults on a guitar-and-bass test signal, P37.2), so Blend 50% is half and
/// half at a matched level, as a pedal's internal gain staging does it, rather than the effect drowning the dry.
fn dry_gain_db(v: Voice) -> f32 {
    match v {
        Voice::Green => 12.5,
        Voice::Blues => 20.5,
        Voice::Gold => 6.5,
        Voice::Germanium => 16.0,
        Voice::Treble => 8.0,
        Voice::Orange => 19.5,
        Voice::Rodent => 17.0,
        Voice::Metal => 18.5,
        Voice::RoundFuzz => 24.5,
        Voice::BigPi => 20.5,
        Voice::Bender => 23.0,
        Voice::OctaveFuzz => 14.5,
        Voice::BassGrind => 22.5,
        Voice::BassMuff => 23.0,
        // (the Bass Driver's own blend, by its circuit, is unchanged)
        Voice::BassDrive => 0.0,
    }
}

/// Where a voice's Blend and Clean low knobs are (they follow its front knobs, before any diodes).
fn blend_at(v: Voice) -> (usize, usize) {
    match v {
        Voice::Green | Voice::Blues | Voice::Gold | Voice::Germanium | Voice::Orange | Voice::Rodent => {
            (3, 4)
        }
        Voice::Treble | Voice::RoundFuzz | Voice::Bender | Voice::OctaveFuzz => (2, 3),
        Voice::Metal => (5, 6),
        Voice::BigPi => (3, 4),
        Voice::BassDrive => (1, 4),
        Voice::BassGrind => (1, 11),
        Voice::BassMuff => (3, 5),
    }
}

/// The shared engine; `set` configures it for its voice.
struct Drive {
    voice: Voice,
    rate: f32,
    /// Conditioning before the gain (up to three biquads, `npre` used).
    pre: [Biquad; 3],
    npre: usize,
    /// The gain path's first-order high-pass (Tube Screamer: 720 Hz; RAT: its two RC branches).
    hp_a: OnePole,
    hp_b: OnePole,
    gain: Smooth,
    gain_b: f32,
    /// A low-pass on the driven signal before clipping (op-amp bandwidth, feedback caps).
    lp_drive: OnePole,
    os: Oversample2,
    sh1: Shaper,
    sh2: Shaper,
    clip1: f32,
    clip2: f32,
    gain2: f32,
    bias: f32,
    bias_f: f32,
    two_stage: bool,
    /// Between the two clipping stages (at twice the rate).
    inter_lp: OnePole,
    inter_hp: OnePole,
    /// Octave fuzz: rectify before the clipper.
    rect_dc: OnePole,
    /// Fuzz Face: the bias shift that follows the level.
    env: f32,
    env_att: f32,
    env_rel: f32,
    dc: DcBlock,
    post: [Biquad; 4],
    npost: usize,
    /// Big Muff / DS-1 tone stack: a low-pass and a high-pass, blended.
    stack_lp: OnePole,
    stack_hp: OnePole,
    stack_t: f32,
    stack: bool,
    /// Clean signal added back (Tube Screamer: all of it before the tone; Klon, Bass Drive: blended).
    clean: Smooth,
    drive_mix: Smooth,
    level: Smooth,
    blend: Blend,
    dry_g: f32,
}

fn new(voice: Voice, rate: f32) -> Box<dyn Pedal> {
    let r2 = rate * 2.0;
    Box::new(Drive {
        voice,
        rate,
        pre: [Biquad::default(); 3],
        npre: 0,
        hp_a: OnePole::new(rate, 720.0),
        hp_b: OnePole::new(rate, 1500.0),
        gain: Smooth::new(rate, 25.0, 1.0),
        gain_b: 0.0,
        lp_drive: OnePole::new(rate, 20000.0),
        os: Oversample2::new(),
        sh1: Shaper::new(Curve::Tanh),
        sh2: Shaper::new(Curve::Tanh),
        clip1: DIODE,
        clip2: DIODE,
        gain2: 1.0,
        bias: 0.0,
        bias_f: 0.0,
        two_stage: false,
        inter_lp: OnePole::new(r2, 6000.0),
        inter_hp: OnePole::new(r2, 60.0),
        rect_dc: OnePole::new(r2, 30.0),
        env: 0.0,
        env_att: 1.0 - (-1.0 / (rate * 0.002)).exp(),
        env_rel: 1.0 - (-1.0 / (rate * 0.09)).exp(),
        dc: DcBlock::new(rate, 8.0),
        post: [Biquad::default(); 4],
        npost: 0,
        stack_lp: OnePole::new(rate, 400.0),
        stack_hp: OnePole::new(rate, 1200.0),
        stack_t: 0.5,
        stack: false,
        clean: Smooth::new(rate, 25.0, 0.0),
        drive_mix: Smooth::new(rate, 25.0, 1.0),
        level: Smooth::new(rate, 25.0, 1.0),
        blend: Blend::new(rate),
        dry_g: db_to_gain(dry_gain_db(voice)),
    })
}

pub fn green(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Green, rate)
}
pub fn blues(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Blues, rate)
}
pub fn gold(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Gold, rate)
}
pub fn germanium(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Germanium, rate)
}
pub fn treble_boost(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Treble, rate)
}
pub fn orange(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Orange, rate)
}
pub fn rodent(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Rodent, rate)
}
pub fn metal(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Metal, rate)
}
pub fn bass_drive(rate: f32) -> Box<dyn Pedal> {
    new(Voice::BassDrive, rate)
}
pub fn round_fuzz(rate: f32) -> Box<dyn Pedal> {
    new(Voice::RoundFuzz, rate)
}
pub fn big_pi(rate: f32) -> Box<dyn Pedal> {
    new(Voice::BigPi, rate)
}
pub fn bender(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Bender, rate)
}
pub fn octave_fuzz(rate: f32) -> Box<dyn Pedal> {
    new(Voice::OctaveFuzz, rate)
}
pub fn bass_grind(rate: f32) -> Box<dyn Pedal> {
    new(Voice::BassGrind, rate)
}
pub fn bass_muff(rate: f32) -> Box<dyn Pedal> {
    new(Voice::BassMuff, rate)
}

impl Drive {
    fn pre(&mut self, fs: &[Filter]) {
        self.npre = fs.len().min(3);
        for (b, f) in self.pre.iter_mut().zip(fs) {
            b.set(self.rate, *f);
        }
    }
    fn post(&mut self, fs: &[Filter]) {
        self.npost = fs.len().min(4);
        for (b, f) in self.post.iter_mut().zip(fs) {
            b.set(self.rate, *f);
        }
    }
    fn curves(&mut self, c1: Curve, c2: Curve) {
        self.sh1.set_curve(c1);
        self.sh2.set_curve(c2);
    }
}

impl Pedal for Drive {
    fn process(&mut self, x: &mut [f32]) {
        let Drive {
            voice,
            pre,
            npre,
            hp_a,
            hp_b,
            gain,
            gain_b,
            lp_drive,
            os,
            sh1,
            sh2,
            clip1,
            clip2,
            gain2,
            bias,
            bias_f,
            two_stage,
            inter_lp,
            inter_hp,
            rect_dc,
            env,
            env_att,
            env_rel,
            dc,
            post,
            npost,
            stack_lp,
            stack_hp,
            stack_t,
            stack,
            clean,
            drive_mix,
            level,
            blend,
            dry_g,
            ..
        } = self;
        let dry_g = *dry_g;
        let voice = *voice;
        for v in x.iter_mut() {
            let input = *v;
            let mut s = input;
            for b in pre.iter_mut().take(*npre) {
                s = b.tick(s);
            }
            let conditioned = s;
            let g = gain.tick();
            // the gain path
            s = match voice {
                Voice::Green => hp_a.hp(s) * g,
                Voice::Rodent => s + hp_a.hp(s) * g + hp_b.hp(s) * *gain_b,
                Voice::Treble => hp_a.hp(s) * g,
                _ => s * g,
            };
            s = lp_drive.lp(s);
            // Fuzz Face: the operating point slides with the level (sputter as notes decay)
            let mut b0 = *bias;
            if voice == Voice::RoundFuzz {
                let a = s.abs();
                *env += (a - *env) * if a > *env { *env_att } else { *env_rel };
                b0 -= 0.18 * (*env / (*clip1 * 3.0)).min(1.0);
            }
            let (c1, c2, g2, two) = (*clip1, *clip2, *gain2, *two_stage);
            let octave = voice == Voice::OctaveFuzz;
            let bf = *bias_f;
            let y = os.run(s, |u| {
                let u = if octave { rect_dc.hp(u.abs() * 1.6) } else { u };
                let mut t = c1 * (sh1.tick(u / c1 + b0) - bf);
                if two {
                    t = inter_hp.hp(inter_lp.lp(t));
                    t = c2 * sh2.tick(t * g2 / c2);
                }
                t
            });
            let mut out = dc.tick(y);
            if voice == Voice::Green {
                // the op-amp's output is its input plus the clipped feedback path
                out += conditioned;
            }
            for b in post.iter_mut().take(*npost) {
                out = b.tick(out);
            }
            if *stack {
                let t = *stack_t;
                out = (stack_lp.lp(out) * (1.0 - t) + stack_hp.hp(out) * t) * 3.2;
            }
            let c = clean.tick();
            let m = drive_mix.tick();
            let wet = out * m + input * c;
            let y = if blend.active() {
                blend.tick(wet, input * dry_g)
            } else {
                wet
            };
            *v = y * level.tick();
        }
    }

    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        let rate = self.rate;
        let r2 = rate * 2.0;
        let lin = |db: f32| db_to_gain(db);
        self.stack = false;
        self.two_stage = false;
        self.bias = 0.0;
        self.clean.target = 0.0;
        self.drive_mix.target = 1.0;
        match self.voice {
            Voice::Green => {
                let (drive, tone, lvl) = (unit10(v[0]), unit10(v[1]), unit10(v[2]));
                let r = 500e3 * audio_taper(drive);
                self.gain.target = 1.0 + (51e3 + r) / 4.7e3;
                self.hp_a.set(rate, 720.0);
                // the 51 pF feedback cap: 61 kHz at minimum drive down to 5.7 kHz at full
                self.lp_drive.set(
                    rate,
                    (1.0 / (std::f32::consts::TAU * (51e3 + r) * 51e-12)).min(20000.0),
                );
                self.curves(Curve::Tanh, Curve::Tanh);
                self.clip1 = DIODE;
                self.pre(&[Filter::HighPass { f: 30.0, q: 0.6 }]);
                self.post(&[
                    Filter::HighShelf {
                        f: 1000.0,
                        db: -11.0 + 15.0 * tone,
                    },
                    Filter::LowPass { f: 7500.0, q: 0.7 },
                    Filter::HighPass { f: 40.0, q: 0.6 },
                ]);
                self.level.target = 0.1 + 1.6 * audio_taper(lvl);
            }
            Voice::Blues => {
                let (gain, tone, lvl) = (unit10(v[0]), unit10(v[1]), unit10(v[2]));
                self.gain.target = 2.0 + 45.0 * audio_taper(gain);
                self.lp_drive.set(rate, 11000.0);
                self.curves(Curve::Asym(0.8), Curve::Asym(0.9));
                self.clip1 = 0.4;
                self.clip2 = 0.62;
                self.two_stage = true;
                self.gain2 = 1.5 + 5.0 * gain;
                self.inter_lp.set(r2, 6500.0);
                self.inter_hp.set(r2, 60.0);
                self.pre(&[
                    Filter::HighPass { f: 70.0, q: 0.6 },
                    Filter::LowPass { f: 9000.0, q: 0.6 },
                ]);
                self.post(&[
                    Filter::HighShelf {
                        f: 1800.0,
                        db: -8.0 + 16.0 * tone,
                    },
                    Filter::LowPass { f: 8500.0, q: 0.7 },
                ]);
                self.level.target = 0.04 + 0.66 * audio_taper(lvl);
            }
            Voice::Gold => {
                let (gain, treble, out) = (unit10(v[0]), unit10(v[1]), unit10(v[2]));
                self.gain.target = 1.0 + 70.0 * audio_taper(gain);
                self.lp_drive.set(rate, 9000.0);
                self.curves(Curve::Soft, Curve::Soft);
                self.clip1 = 0.22;
                self.pre(&[
                    Filter::HighPass { f: 160.0, q: 0.6 },
                    Filter::Peak {
                        f: 1000.0,
                        q: 0.7,
                        db: 4.0,
                    },
                ]);
                self.post(&[Filter::HighShelf {
                    f: 1200.0,
                    db: -10.0 + 16.0 * treble,
                }]);
                // the dual-gang gain pot: the more drive, the less clean
                self.clean.target = 1.0 - 0.6 * gain;
                self.drive_mix.target = 0.9;
                self.level.target = 0.15 + 2.0 * audio_taper(out);
            }
            Voice::Germanium => {
                let (gain, tone, lvl) = (unit10(v[0]), unit10(v[1]), unit10(v[2]));
                self.gain.target = 2.0 + 80.0 * audio_taper(gain);
                self.lp_drive.set(rate, 9000.0);
                self.curves(Curve::Asym(0.5), Curve::Tanh);
                self.clip1 = 0.3;
                self.pre(&[
                    Filter::HighPass { f: 90.0, q: 0.6 },
                    Filter::LowPass { f: 5000.0, q: 0.6 },
                ]);
                self.post(&[Filter::LowPass {
                    f: sweep(1500.0, 9000.0, tone),
                    q: 0.6,
                }]);
                self.level.target = 0.07 + 1.0 * audio_taper(lvl);
            }
            Voice::Treble => {
                let (boost, range) = (unit10(v[0]), unit10(v[1]));
                // a germanium transistor stage after a small coupling cap: the range knob lowers its corner
                self.hp_a.set(rate, sweep(1600.0, 220.0, range));
                self.gain.target = 1.5 + 28.0 * audio_taper(boost);
                self.lp_drive.set(rate, 12000.0);
                self.curves(Curve::Asym(0.45), Curve::Tanh);
                self.clip1 = 0.5;
                self.bias = 0.15;
                self.post(&[Filter::LowPass { f: 9000.0, q: 0.6 }]);
                self.npre = 0;
                self.level.target = 0.57;
            }
            Voice::Orange => {
                let (dist, tone, lvl) = (unit10(v[0]), unit10(v[1]), unit10(v[2]));
                // the transistor booster then the op-amp: a lot of gain even at zero
                self.gain.target = 6.0 + 320.0 * audio_taper(dist);
                // the 2.2k / 10 nF network before the diodes: 7.2 kHz
                self.lp_drive.set(rate, 7200.0);
                self.curves(Curve::Tanh, Curve::Tanh);
                self.clip1 = DIODE;
                self.pre(&[Filter::HighPass { f: 25.0, q: 0.6 }]);
                self.npost = 0;
                self.stack = true;
                self.stack_lp.set(rate, 234.0);
                self.stack_hp.set(rate, 1060.0);
                self.stack_t = tone;
                self.level.target = 0.055 + 1.0 * audio_taper(lvl);
            }
            Voice::Rodent => {
                let (dist, filter, vol) = (unit10(v[0]), unit10(v[1]), unit10(v[2]));
                let r = 100e3 * audio_taper(dist);
                // two RC branches to ground: 560 Ω / 4.7 µF (60 Hz) and 47 Ω / 2.2 µF (1.5 kHz)
                self.hp_a.set(rate, 60.0);
                self.hp_b.set(rate, 1540.0);
                self.gain.target = r / 560.0;
                self.gain_b = r / 47.0;
                // the LM308's gain-bandwidth: the more gain, the less top before the diodes
                let g_eff = (r / 43.0).max(1.0);
                self.lp_drive.set(rate, (1.0e6 / g_eff).clamp(900.0, 20000.0));
                self.curves(Curve::Hard, Curve::Tanh);
                self.clip1 = DIODE;
                self.pre(&[Filter::HighPass { f: 30.0, q: 0.6 }]);
                // the Filter pot: 1.5k + 100k against 3.3 nF, 32 kHz down to 475 Hz
                let fc = 1.0 / (std::f32::consts::TAU * (1.5e3 + 100e3 * filter) * 3.3e-9);
                self.post(&[
                    Filter::LowPass {
                        f: fc.min(19000.0),
                        q: 0.5,
                    },
                    Filter::LowPass { f: 12000.0, q: 0.6 },
                ]);
                self.level.target = 0.075 + 1.28 * audio_taper(vol);
            }
            Voice::Metal => {
                let (gain, lvl) = (unit10(v[0]), unit10(v[4]));
                self.gain.target = 10.0 + 200.0 * audio_taper(gain);
                self.lp_drive.set(rate, 9000.0);
                self.curves(Curve::Tanh, Curve::Hard);
                self.clip1 = DIODE;
                self.clip2 = DIODE;
                self.two_stage = true;
                self.gain2 = 4.0 + 18.0 * gain;
                self.inter_lp.set(r2, 5000.0);
                self.inter_hp.set(r2, 120.0);
                self.pre(&[
                    Filter::HighPass { f: 110.0, q: 0.7 },
                    Filter::Peak {
                        f: 1200.0,
                        q: 0.8,
                        db: 6.0,
                    },
                ]);
                self.post(&[
                    Filter::LowShelf { f: 120.0, db: v[1] },
                    Filter::Peak {
                        f: if v[7] > 0.0 {
                            v[7].clamp(200.0, 5000.0)
                        } else {
                            900.0
                        },
                        q: 0.8,
                        db: v[2],
                    },
                    Filter::HighShelf { f: 3000.0, db: v[3] },
                    Filter::LowPass { f: 7000.0, q: 0.7 },
                ]);
                self.level.target = 0.06 + 1.0 * audio_taper(lvl);
            }
            Voice::BassDrive => {
                let (gain, blend, tone, lvl) = (
                    unit10(v[0]),
                    (v[1] / 100.0).clamp(0.0, 1.0),
                    unit10(v[2]),
                    unit10(v[3]),
                );
                self.gain.target = 1.5 + 40.0 * audio_taper(gain);
                self.lp_drive.set(rate, 9000.0);
                self.curves(Curve::Asym(0.7), Curve::Tanh);
                self.clip1 = 0.5;
                // (P37.2: 40 Hz to 18, so a 5-string's low B keeps its fundamental)
                self.pre(&[Filter::HighPass { f: 18.0, q: 0.6 }]);
                self.post(&[
                    Filter::LowPass { f: 3200.0, q: 0.8 },
                    Filter::LowShelf { f: 90.0, db: 4.0 },
                    Filter::LowShelf {
                        f: 250.0,
                        db: (0.5 - tone) * 12.0,
                    },
                    Filter::HighShelf {
                        f: 2500.0,
                        db: (tone - 0.5) * 12.0,
                    },
                ]);
                if v[4] >= 0.5 {
                    // Clean low: the blend moves to the crossover
                    self.clean.target = 0.0;
                    self.drive_mix.target = 0.8;
                    self.blend.set(blend, v[4]);
                } else {
                    self.clean.target = 1.0 - blend;
                    self.drive_mix.target = blend * 0.8;
                    self.blend.set(1.0, 0.0);
                }
                self.level.target = 0.2 + 1.8 * audio_taper(lvl);
            }
            Voice::BassGrind => {
                let (drive, lvl) = (unit10(v[0]), unit10(v[2]));
                let grunt = [6.0, 0.0, -5.0][(v[7].round().clamp(0.0, 2.0)) as usize];
                let attack = [-6.0, 0.0, 6.0][(v[8].round().clamp(0.0, 2.0)) as usize];
                let lo_f = [250.0, 500.0, 1000.0][(v[9].round().clamp(0.0, 2.0)) as usize];
                let hi_f = [750.0, 1500.0, 3000.0][(v[10].round().clamp(0.0, 2.0)) as usize];
                // the Microtubes' CMOS stages: a lot of gain, hard tops, the bass shaped before the clipper
                self.gain.target = 3.0 + 160.0 * audio_taper(drive);
                self.lp_drive.set(rate, 6500.0);
                self.curves(Curve::Tanh, Curve::Hard);
                self.clip1 = 0.45;
                self.clip2 = 0.6;
                self.two_stage = true;
                self.gain2 = 2.0 + 4.0 * drive;
                self.inter_lp.set(r2, 5000.0);
                self.inter_hp.set(r2, 35.0);
                self.pre(&[
                    Filter::HighPass { f: 25.0, q: 0.6 },
                    Filter::LowShelf { f: 160.0, db: grunt },
                    Filter::HighShelf {
                        f: 2000.0,
                        db: attack,
                    },
                ]);
                self.post(&[
                    Filter::LowShelf { f: 100.0, db: v[3] },
                    Filter::Peak {
                        f: lo_f,
                        q: 0.8,
                        db: v[4],
                    },
                    Filter::Peak {
                        f: hi_f,
                        q: 0.8,
                        db: v[5],
                    },
                    Filter::HighShelf {
                        f: 5000.0,
                        db: v[6] - 4.0,
                    },
                ]);
                self.level.target = 0.025 + 0.36 * audio_taper(lvl);
            }
            Voice::BassMuff => {
                let (sustain, tone, vol) = (unit10(v[0]), unit10(v[1]), unit10(v[2]));
                let boost = v[4] >= 0.5;
                self.gain.target = 3.0 + 70.0 * audio_taper(sustain);
                self.lp_drive.set(rate, 7000.0);
                self.curves(Curve::Tanh, Curve::Tanh);
                self.clip1 = DIODE;
                self.clip2 = DIODE;
                self.two_stage = true;
                self.gain2 = 30.0;
                self.inter_lp.set(r2, 4500.0);
                // the bass version's coupling caps: the lows go through the clippers
                self.inter_hp.set(r2, 35.0);
                self.pre(&[Filter::HighPass { f: 25.0, q: 0.6 }]);
                self.post(&[
                    Filter::LowPass { f: 5000.0, q: 0.6 },
                    Filter::LowShelf {
                        f: 100.0,
                        db: if boost { 6.0 } else { 0.0 },
                    },
                ]);
                self.stack = true;
                self.stack_lp.set(rate, 300.0);
                self.stack_hp.set(rate, 1100.0);
                self.stack_t = tone;
                self.level.target = 0.04 + 0.74 * audio_taper(vol);
            }
            // (the fuzzes' levels: 2-4 dB over the drives at noon, as squashed as they are; they were 7-10 over)
            Voice::RoundFuzz => {
                let (fuzz, vol) = (unit10(v[0]), unit10(v[1]));
                self.gain.target = 2.0 + 400.0 * audio_taper(fuzz);
                // the fuzz's low input impedance loads the pickup: the top rolls off
                self.lp_drive.set(rate, 9000.0);
                self.curves(Curve::Asym(0.35), Curve::Tanh);
                self.clip1 = 0.45;
                self.bias = 0.25;
                self.pre(&[
                    Filter::LowPass { f: 4500.0, q: 0.6 },
                    Filter::HighPass { f: 70.0, q: 0.6 },
                ]);
                self.post(&[
                    Filter::LowPass { f: 3800.0, q: 0.6 },
                    Filter::LowPass { f: 7000.0, q: 0.6 },
                    Filter::HighPass { f: 80.0, q: 0.6 },
                ]);
                self.level.target = 0.019 + 0.3 * audio_taper(vol);
            }
            Voice::BigPi => {
                let (sustain, tone, vol) = (unit10(v[0]), unit10(v[1]), unit10(v[2]));
                self.gain.target = 3.0 + 70.0 * audio_taper(sustain);
                self.lp_drive.set(rate, 8000.0);
                self.curves(Curve::Tanh, Curve::Tanh);
                self.clip1 = DIODE;
                self.clip2 = DIODE;
                self.two_stage = true;
                self.gain2 = 30.0;
                self.inter_lp.set(r2, 4500.0);
                self.inter_hp.set(r2, 100.0);
                self.pre(&[Filter::HighPass { f: 100.0, q: 0.6 }]);
                self.post(&[Filter::LowPass { f: 5000.0, q: 0.6 }]);
                self.stack = true;
                self.stack_lp.set(rate, 400.0);
                self.stack_hp.set(rate, 1200.0);
                self.stack_t = tone;
                self.level.target = 0.028 + 0.52 * audio_taper(vol);
            }
            Voice::Bender => {
                let (attack, lvl) = (unit10(v[0]), unit10(v[1]));
                self.gain.target = 3.0 + 300.0 * audio_taper(attack);
                self.lp_drive.set(rate, 9000.0);
                self.curves(Curve::Asym(0.5), Curve::Asym(0.6));
                self.clip1 = 0.4;
                self.clip2 = 0.55;
                self.bias = 0.1;
                self.two_stage = true;
                self.gain2 = 3.0;
                self.inter_lp.set(r2, 7000.0);
                self.inter_hp.set(r2, 80.0);
                self.pre(&[
                    Filter::HighPass { f: 150.0, q: 0.6 },
                    Filter::Peak {
                        f: 1000.0,
                        q: 0.7,
                        db: 3.0,
                    },
                ]);
                self.post(&[
                    Filter::LowPass { f: 5000.0, q: 0.6 },
                    Filter::LowPass { f: 9000.0, q: 0.7 },
                ]);
                self.level.target = 0.025 + 0.38 * audio_taper(lvl);
            }
            Voice::OctaveFuzz => {
                let (fuzz, vol) = (unit10(v[0]), unit10(v[1]));
                self.gain.target = 2.0 + 50.0 * audio_taper(fuzz);
                self.lp_drive.set(rate, 9000.0);
                self.curves(Curve::Tanh, Curve::Tanh);
                self.clip1 = 0.4;
                self.rect_dc.set(r2, 40.0);
                // fundamentals in, so the rectifier doubles one note rather than a chord's mess
                self.pre(&[
                    Filter::HighPass { f: 120.0, q: 0.6 },
                    Filter::LowPass { f: 3000.0, q: 0.6 },
                ]);
                self.post(&[
                    Filter::LowPass { f: 6000.0, q: 0.6 },
                    Filter::HighPass { f: 60.0, q: 0.6 },
                ]);
                self.level.target = 0.045 + 0.68 * audio_taper(vol);
            }
        }
        // the Blend and Clean low (the Bass Driver set its own above)
        let (bi, ci) = blend_at(self.voice);
        if self.voice != Voice::BassDrive {
            self.blend.set((v[bi] / 100.0).clamp(0.0, 1.0), v[ci]);
        }
        // the clipping diodes inside (`catalog::DIODES`; after the front knobs): (index, the pedal's own pair)
        let diodes = match self.voice {
            Voice::Green | Voice::Orange | Voice::Rodent => Some(0),
            Voice::Blues => Some(1),
            Voice::Gold | Voice::Germanium => Some(3),
            _ => None,
        };
        if let Some(stock) = diodes {
            let d = v[ci + 1].round().clamp(0.0, 3.0) as usize;
            if d != stock {
                let (curve, clip) = diode(d);
                // the same loudness for a swap, except LEDs: they clip later and come out louder, as the real mod does
                let comp = self.clip1 / clip;
                self.level.target *= if d == 2 { 1.0 } else { comp };
                self.sh1.set_curve(curve);
                self.clip1 = clip;
            }
        }
        // the static bias offset, so silence stays silent
        self.bias_f = self.sh1.curve.f(self.bias as f64) as f32;
        let _ = lin;
    }

    fn reset(&mut self) {
        for b in self.pre.iter_mut().chain(self.post.iter_mut()) {
            b.reset();
        }
        self.hp_a.reset();
        self.hp_b.reset();
        self.lp_drive.reset();
        self.os.reset();
        self.sh1.reset_at(self.bias);
        self.sh2.reset();
        self.inter_lp.reset();
        self.inter_hp.reset();
        self.rect_dc.reset();
        self.stack_lp.reset();
        self.stack_hp.reset();
        self.dc.reset();
        self.env = 0.0;
        self.gain.snap();
        self.clean.snap();
        self.drive_mix.snap();
        self.level.snap();
        self.blend.reset();
    }
}
