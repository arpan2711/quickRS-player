//! Amps: preamp stages, the passive tone stack, a power stage.
//!
//! - **Preamp**: triode stages as gain, a Miller-capacitance low-pass, an asymmetric soft clipper (grid conduction
//!   on one side, cutoff on the other: `Curve::Asym`) and a coupling-cap high-pass, at twice the rate with ADAA.
//!   The cascade and its filters are each amp's (two stages and a cathode follower for the Marshalls, four for the
//!   Rectifier with a tight high-pass in front).
//! - **Tone stack**: the passive treble/mid/bass network solved in closed form by Yeh and Smith, "Discretization of
//!   the '59 Fender Bassman Tone Stack" (DAFx-06), discretized by the bilinear transform, with each amp's
//!   component values (as published in the Faust `tonestacks` library). The bass pot is log taper.
//! - **Power stage**: presence and depth (resonance) as shelves into a push-pull clipper (single-ended class A for
//!   the AC30), its drive set by the master knob and pulled down by supply sag (an envelope), then the output
//!   transformer's band limits.
//! - **Back panel** (D45): sag (how far the supply droops; a tube rectifier droops further and recovers slower),
//!   bias (cold: a crossover notch around zero; hot: earlier, lopsided clipping), power tubes (EL34, 6L6, KT88, EL84,
//!   6V6: the stage's headroom, curve and EQ, relative to the amp's own) and a power master (drive into the stage).
//! - **Bass** (P37.2): the SVT (with its Ultra Lo and Ultra Hi switches), the B-15 Portaflex (a small tube head, the
//!   Bassman stack, 6V6s), a solid-state hybrid head (a clean preamp into a hard-clipping boost, an active EQ with a
//!   semi-parametric mid and a Contour scoop, a transformerless power stage) and an Orange-style all-tube head. Their
//!   input and output-transformer high-passes sit at 15-20 Hz, so a 5-string's low B (31 Hz) keeps its fundamental.

use super::dsp::{
    Biquad, Curve, Filter, Lfo, OnePole, Oversample2, Shaper, Smooth, audio_taper, db_to_gain, flush, sweep,
    unit10,
};
use super::{MAX_KNOBS, Pedal};

/// The passive tone stack: a third-order IIR from Yeh and Smith's analog transfer function.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToneStack {
    b: [f64; 4],
    a: [f64; 4],
    z: [f64; 3],
}

/// Component values (R1, R2, R3, R4 in ohms; C1, C2, C3 in farads).
#[derive(Clone, Copy, Debug)]
pub struct Stack {
    pub r1: f64,
    pub r2: f64,
    pub r3: f64,
    pub r4: f64,
    pub c1: f64,
    pub c2: f64,
    pub c3: f64,
}

pub const TWIN: Stack = Stack {
    r1: 250e3,
    r2: 250e3,
    r3: 10e3,
    r4: 100e3,
    c1: 120e-12,
    c2: 100e-9,
    c3: 47e-9,
};
pub const BASSMAN: Stack = Stack {
    r1: 250e3,
    r2: 1e6,
    r3: 25e3,
    r4: 56e3,
    c1: 250e-12,
    c2: 20e-9,
    c3: 20e-9,
};
pub const JTM45: Stack = Stack {
    r1: 250e3,
    r2: 1e6,
    r3: 25e3,
    r4: 33e3,
    c1: 270e-12,
    c2: 22e-9,
    c3: 22e-9,
};
pub const JCM800: Stack = Stack {
    r1: 220e3,
    r2: 1e6,
    r3: 22e3,
    r4: 33e3,
    c1: 470e-12,
    c2: 22e-9,
    c3: 22e-9,
};
pub const MESA: Stack = Stack {
    r1: 250e3,
    r2: 250e3,
    r3: 25e3,
    r4: 100e3,
    c1: 250e-12,
    c2: 100e-9,
    c3: 47e-9,
};
pub const AC30: Stack = Stack {
    r1: 1e6,
    r2: 1e6,
    r3: 10e3,
    r4: 100e3,
    c1: 50e-12,
    c2: 22e-9,
    c3: 22e-9,
};
pub const AMPEG: Stack = Stack {
    r1: 250e3,
    r2: 1e6,
    r3: 25e3,
    r4: 32e3,
    c1: 470e-12,
    c2: 22e-9,
    c3: 22e-9,
};

impl Stack {
    /// The analog transfer function's coefficients (b1..b3 over a0..a3; b0 = 0) for treble `t`, mid `m` and bass
    /// `bass` in 0..1 (the bass pot log taper applied here).
    pub fn analog(&self, t: f64, m: f64, bass: f64) -> ([f64; 4], [f64; 4]) {
        let Stack {
            r1,
            r2,
            r3,
            r4,
            c1,
            c2,
            c3,
        } = *self;
        let l = ((bass.clamp(0.0, 1.0) - 1.0) * 3.4).exp();
        let (t, m) = (t.clamp(0.0, 1.0), m.clamp(0.0, 1.0));
        let b1 = t * c1 * r1 + m * c3 * r3 + l * (c1 * r2 + c2 * r2) + (c1 * r3 + c2 * r3);
        let b2 = t * (c1 * c2 * r1 * r4 + c1 * c3 * r1 * r4)
            - m * m * (c1 * c3 * r3 * r3 + c2 * c3 * r3 * r3)
            + m * (c1 * c3 * r1 * r3 + c1 * c3 * r3 * r3 + c2 * c3 * r3 * r3)
            + l * (c1 * c2 * r1 * r2 + c1 * c2 * r2 * r4 + c1 * c3 * r2 * r4)
            + l * m * (c1 * c3 * r2 * r3 + c2 * c3 * r2 * r3)
            + (c1 * c2 * r1 * r3 + c1 * c2 * r3 * r4 + c1 * c3 * r3 * r4);
        let b3 = l * m * (c1 * c2 * c3 * r1 * r2 * r3 + c1 * c2 * c3 * r2 * r3 * r4)
            - m * m * (c1 * c2 * c3 * r1 * r3 * r3 + c1 * c2 * c3 * r3 * r3 * r4)
            + m * (c1 * c2 * c3 * r1 * r3 * r3 + c1 * c2 * c3 * r3 * r3 * r4)
            + t * c1 * c2 * c3 * r1 * r3 * r4
            - t * m * c1 * c2 * c3 * r1 * r3 * r4
            + t * l * c1 * c2 * c3 * r1 * r2 * r4;
        let a0 = 1.0;
        let a1 = (c1 * r1 + c1 * r3 + c2 * r3 + c2 * r4 + c3 * r4) + m * c3 * r3 + l * (c1 * r2 + c2 * r2);
        let a2 = m * (c1 * c3 * r1 * r3 - c2 * c3 * r3 * r4 + c1 * c3 * r3 * r3 + c2 * c3 * r3 * r3)
            + l * m * (c1 * c3 * r2 * r3 + c2 * c3 * r2 * r3)
            - m * m * (c1 * c3 * r3 * r3 + c2 * c3 * r3 * r3)
            + l * (c1 * c2 * r2 * r4 + c1 * c2 * r1 * r2 + c1 * c3 * r2 * r4 + c2 * c3 * r2 * r4)
            + (c1 * c2 * r1 * r4
                + c1 * c3 * r1 * r4
                + c1 * c2 * r3 * r4
                + c1 * c2 * r1 * r3
                + c1 * c3 * r3 * r4
                + c2 * c3 * r3 * r4);
        let a3 = l * m * (c1 * c2 * c3 * r1 * r2 * r3 + c1 * c2 * c3 * r2 * r3 * r4)
            - m * m * (c1 * c2 * c3 * r1 * r3 * r3 + c1 * c2 * c3 * r3 * r3 * r4)
            + m * (c1 * c2 * c3 * r3 * r3 * r4 + c1 * c2 * c3 * r1 * r3 * r3 - c1 * c2 * c3 * r1 * r3 * r4)
            + l * c1 * c2 * c3 * r1 * r2 * r4
            + c1 * c2 * c3 * r1 * r3 * r4;
        ([0.0, b1, b2, b3], [a0, a1, a2, a3])
    }

    /// |H(j 2π f)| in dB (for the big view's curve and the tests).
    pub fn response_db(&self, f: f64, t: f64, m: f64, bass: f64) -> f64 {
        let (b, a) = self.analog(t, m, bass);
        let w = std::f64::consts::TAU * f;
        let (nr, ni) = (-b[2] * w * w, b[1] * w - b[3] * w * w * w);
        let (dr, di) = (a[0] - a[2] * w * w, a[1] * w - a[3] * w * w * w);
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di) + 1e-30).log10()
    }
}

impl ToneStack {
    /// New coefficients (keeps the state): the bilinear transform of `stack` at `rate`.
    pub fn set(&mut self, rate: f64, stack: &Stack, t: f64, m: f64, bass: f64) {
        let (b, a) = stack.analog(t, m, bass);
        let c = 2.0 * rate;
        let (c2, c3) = (c * c, c * c * c);
        // multiply through by (1 + z^-1)^3 with s = c (1 - z^-1) / (1 + z^-1)
        let nb = [
            b[1] * c + b[2] * c2 + b[3] * c3,
            b[1] * c - b[2] * c2 - 3.0 * b[3] * c3,
            -b[1] * c - b[2] * c2 + 3.0 * b[3] * c3,
            -b[1] * c + b[2] * c2 - b[3] * c3,
        ];
        let na = [
            a[0] + a[1] * c + a[2] * c2 + a[3] * c3,
            3.0 * a[0] + a[1] * c - a[2] * c2 - 3.0 * a[3] * c3,
            3.0 * a[0] - a[1] * c - a[2] * c2 + 3.0 * a[3] * c3,
            a[0] - a[1] * c + a[2] * c2 - a[3] * c3,
        ];
        let k = 1.0 / na[0];
        for i in 0..4 {
            self.b[i] = nb[i] * k;
            self.a[i] = na[i] * k;
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[1] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[2] * y + self.z[2];
        self.z[2] = self.b[3] * x - self.a[3] * y;
        for z in &mut self.z {
            if z.abs() < 1e-30 {
                *z = 0.0;
            }
        }
        y as f32
    }

    pub fn reset(&mut self) {
        self.z = [0.0; 3];
    }
}

/// One triode stage at the oversampled rate.
#[derive(Clone, Copy, Debug)]
struct Stage {
    gain: f32,
    /// A partly bypassed cathode: below `cathode`'s corner the stage has only `low_keep` of its gain, which is
    /// what keeps a British or modern amp's lows tight when it distorts.
    cathode: OnePole,
    low_keep: f32,
    miller: OnePole,
    shaper: Shaper,
    bias: f32,
    bias_f: f32,
    coupling: OnePole,
}

impl Stage {
    fn new(rate2: f32) -> Stage {
        Stage {
            gain: 1.0,
            cathode: OnePole::new(rate2, 120.0),
            low_keep: 1.0,
            miller: OnePole::new(rate2, 12000.0),
            shaper: Shaper::new(Curve::Asym(0.75)),
            bias: 0.1,
            bias_f: 0.0,
            coupling: OnePole::new(rate2, 30.0),
        }
    }
    fn tune(&mut self, rate2: f32, gain: f32, curve: Curve, bias: f32, lp: f32, hp: f32) {
        self.gain = gain;
        self.shaper.set_curve(curve);
        self.bias = bias;
        self.bias_f = curve.f(bias as f64) as f32;
        self.miller.set(rate2, lp);
        self.coupling.set(rate2, hp);
    }
    /// The cathode's low shelf: lows below `f` keep `keep` of the gain.
    fn cathode(&mut self, rate2: f32, f: f32, keep: f32) {
        self.cathode.set(rate2, f);
        self.low_keep = keep;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let x = if self.low_keep < 1.0 {
            x - (1.0 - self.low_keep) * self.cathode.lp(x)
        } else {
            x
        };
        let s = self.miller.lp(x * self.gain);
        let y = self.shaper.tick(s + self.bias) - self.bias_f;
        self.coupling.hp(y)
    }
    fn reset(&mut self) {
        self.cathode.reset();
        self.miller.reset();
        self.shaper.reset_at(self.bias);
        self.coupling.reset();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Voice {
    Clean,
    Tweed,
    Chime,
    Crunch,
    Lead,
    Modern,
    Bass,
    B15,
    BassSs,
    BassTube,
    /// P41.2 (D167): the Mark (a cascaded lead preamp with its tone stack early and a five-band graphic EQ), the
    /// SLO / 5150-style hot-rodded lead, the Orange-style crunch, the Hiwatt-style clean-loud head.
    Cali,
    Snake,
    Tangerine,
    Surrey,
}

/// The Mark's five-band graphic EQ (Hz), in the power amp's path as on the real one.
pub const GEQ_HZ: [f32; 5] = [80.0, 240.0, 750.0, 2200.0, 6600.0];

/// Where the tone stack sits.
#[derive(Clone, Copy, Debug, PartialEq)]
enum StackAt {
    /// Between the first and second stage (Fender, Vox, Ampeg): runs at the oversampled rate.
    AfterFirst,
    /// After the preamp (Marshall, Mesa).
    AfterPreamp,
    /// No stack (the tweed's single tone knob).
    None,
}

struct Amp {
    /// The Twin's reverb and vibrato (none on the others).
    extra: Option<Box<Extras>>,
    voice: Voice,
    rate: f32,
    input_hp: OnePole,
    pre: [Biquad; 2],
    npre: usize,
    stages: [Stage; 4],
    nstages: usize,
    stack_at: StackAt,
    stack: Stack,
    ts: ToneStack,
    ts_gain: f32,
    os_pre: Oversample2,
    /// After the preamp, at the base rate: tweed tone, bass amp mid, Vox brilliance.
    post: [Biquad; 3],
    npost: usize,
    /// The Mark's graphic EQ (none on the others).
    geq: [Biquad; 5],
    ngeq: usize,
    /// Power stage.
    presence: Biquad,
    depth: Biquad,
    drive: Smooth,
    sag: f32,
    sag_env: f32,
    sag_att: f32,
    sag_rel: f32,
    os_pow: Oversample2,
    pow_sh: Shaper,
    /// The back panel's power tubes against the amp's own: a voicing peak for the chosen tube and the stock one's
    /// undone (`ntube` used; none when the tubes are stock).
    tube_eq: [Biquad; 2],
    ntube: usize,
    /// Bias: cold opens a crossover notch around zero (`xover` deep, `xover_w` wide); hot shifts the operating
    /// point (`pow_bias`, its static output `pow_bias_f` taken off so silence stays silent).
    xover: f32,
    xover_w: f32,
    pow_bias: f32,
    pow_bias_f: f32,
    cut: OnePole,
    xf_hp: OnePole,
    xf_lp: Biquad,
    level: Smooth,
}

/// A power tube's voicing: drive into the stage (headroom), its clipping curve, an EQ the stage adds.
struct Tube {
    drive: f32,
    curve: Curve,
    eq: Filter,
}

/// In `catalog::TUBES` order: EL34, 6L6, KT88, EL84, 6V6.
fn tube(i: usize) -> Tube {
    match i {
        // British: mid-forward, breaks up early
        0 => Tube {
            drive: 1.25,
            curve: Curve::Tanh,
            eq: Filter::Peak {
                f: 800.0,
                q: 0.7,
                db: 2.0,
            },
        },
        // American: more headroom, tighter, scooped mids
        1 => Tube {
            drive: 0.85,
            curve: Curve::Tanh,
            eq: Filter::Peak {
                f: 600.0,
                q: 0.6,
                db: -2.0,
            },
        },
        // KT88 / 6550: big clean headroom, a firm low end
        2 => Tube {
            drive: 0.6,
            curve: Curve::Tanh,
            eq: Filter::LowShelf { f: 110.0, db: 2.0 },
        },
        // EL84 (class A, cathode biased): early, lopsided, chimey upper mids
        3 => Tube {
            drive: 1.4,
            curve: Curve::Asym(0.75),
            eq: Filter::Peak {
                f: 1800.0,
                q: 0.8,
                db: 2.5,
            },
        },
        // 6V6: small, early, loose breakup
        _ => Tube {
            drive: 1.5,
            curve: Curve::Asym(0.85),
            eq: Filter::Peak {
                f: 500.0,
                q: 0.7,
                db: 1.5,
            },
        },
    }
}

fn inverse(f: Filter) -> Filter {
    match f {
        Filter::Peak { f, q, db } => Filter::Peak { f, q, db: -db },
        Filter::LowShelf { f, db } => Filter::LowShelf { f, db: -db },
        Filter::HighShelf { f, db } => Filter::HighShelf { f, db: -db },
        other => other,
    }
}

/// The Twin's own reverb and vibrato channel (P37.6c): a spring tank after the amp (ours; on the real one it sits
/// before the power stage) and an optical-style tremolo. Allocated with the amp, never on the audio thread.
struct Extras {
    spring: Box<dyn Pedal>,
    buf: [f32; 128],
    reverb: bool,
    trem: Lfo,
    trem_depth: Smooth,
}

impl Extras {
    fn new(rate: f32) -> Box<Extras> {
        Box::new(Extras {
            spring: super::reverb::spring(rate),
            buf: [0.0; 128],
            reverb: false,
            trem: Lfo::default(),
            trem_depth: Smooth::new(rate, 30.0, 0.0),
        })
    }

    fn set(&mut self, rate: f32, reverb: f32, speed: f32, intensity: f32) {
        let mut k = [0.0f32; MAX_KNOBS];
        // (dwell and tone where the Twin's tank sits; the Reverb knob is the mix)
        k[0] = 4.5;
        k[1] = 6.0;
        k[2] = 70.0 * reverb;
        self.spring.set(&k);
        self.reverb = reverb > 0.0;
        self.trem.set(rate, sweep(1.5, 10.0, speed));
        self.trem_depth.target = 0.9 * intensity;
    }

    fn run(&mut self, x: &mut [f32]) {
        if self.trem_depth.target > 0.0 || self.trem_depth.tick() > 1e-4 {
            for v in x.iter_mut() {
                let p = self.trem.step();
                let d = self.trem_depth.tick();
                *v *= 1.0 - d * (0.5 + 0.5 * Lfo::sine(p));
            }
        }
        if self.reverb {
            for c in x.chunks_mut(128) {
                let b = &mut self.buf[..c.len()];
                b.copy_from_slice(c);
                self.spring.process(b);
                for (o, w) in c.iter_mut().zip(b.iter()) {
                    *o += *w;
                }
            }
        }
    }
}

fn new(voice: Voice, rate: f32) -> Box<dyn Pedal> {
    let r2 = rate * 2.0;
    Box::new(Amp {
        extra: (voice == Voice::Clean).then(|| Extras::new(rate)),
        voice,
        rate,
        input_hp: OnePole::new(rate, 25.0),
        pre: [Biquad::default(); 2],
        npre: 0,
        stages: [Stage::new(r2); 4],
        nstages: 2,
        stack_at: StackAt::AfterPreamp,
        stack: BASSMAN,
        ts: ToneStack::default(),
        ts_gain: 1.0,
        os_pre: Oversample2::new(),
        post: [Biquad::default(); 3],
        npost: 0,
        geq: [Biquad::default(); 5],
        ngeq: 0,
        presence: Biquad::new(rate, Filter::HighShelf { f: 3500.0, db: 0.0 }),
        depth: Biquad::new(
            rate,
            Filter::Peak {
                f: 90.0,
                q: 0.8,
                db: 0.0,
            },
        ),
        drive: Smooth::new(rate, 30.0, 1.0),
        sag: 0.3,
        sag_env: 0.0,
        sag_att: 1.0 - (-1.0 / (rate * 0.012)).exp(),
        sag_rel: 1.0 - (-1.0 / (rate * 0.18)).exp(),
        os_pow: Oversample2::new(),
        pow_sh: Shaper::new(Curve::Tanh),
        tube_eq: [Biquad::default(); 2],
        ntube: 0,
        xover: 0.0,
        xover_w: 0.015,
        pow_bias: 0.0,
        pow_bias_f: 0.0,
        cut: OnePole::new(r2, 18000.0),
        xf_hp: OnePole::new(rate, 55.0),
        xf_lp: Biquad::new(rate, Filter::LowPass { f: 9000.0, q: 0.7 }),
        level: Smooth::new(rate, 30.0, 1.0),
    })
}

pub fn clean(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Clean, rate)
}
pub fn tweed(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Tweed, rate)
}
pub fn chime(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Chime, rate)
}
pub fn crunch(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Crunch, rate)
}
pub fn lead(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Lead, rate)
}
pub fn modern(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Modern, rate)
}
pub fn bass(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Bass, rate)
}
pub fn b15(rate: f32) -> Box<dyn Pedal> {
    new(Voice::B15, rate)
}
pub fn bass_ss(rate: f32) -> Box<dyn Pedal> {
    new(Voice::BassSs, rate)
}
pub fn bass_tube(rate: f32) -> Box<dyn Pedal> {
    new(Voice::BassTube, rate)
}
pub fn cali(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Cali, rate)
}
pub fn snake(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Snake, rate)
}
pub fn tangerine(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Tangerine, rate)
}
pub fn surrey(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Surrey, rate)
}

impl Amp {
    fn pre(&mut self, fs: &[Filter]) {
        self.npre = fs.len().min(2);
        for (b, f) in self.pre.iter_mut().zip(fs) {
            b.set(self.rate, *f);
        }
    }
    fn post(&mut self, fs: &[Filter]) {
        self.npost = fs.len().min(3);
        for (b, f) in self.post.iter_mut().zip(fs) {
            b.set(self.rate, *f);
        }
    }
    fn stack(&mut self, at: StackAt, stack: Stack, t: f32, m: f32, b: f32, makeup_db: f32) {
        self.stack_at = at;
        self.stack = stack;
        let r = if at == StackAt::AfterFirst {
            self.rate * 2.0
        } else {
            self.rate
        };
        self.ts.set(r as f64, &stack, t as f64, m as f64, b as f64);
        self.ts_gain = db_to_gain(makeup_db);
    }
    fn power(&mut self, curve: Curve, drive: f32, presence_db: f32, depth_db: f32, xf_lp: f32) {
        self.pow_sh.set_curve(curve);
        self.drive.target = drive;
        self.presence.set(
            self.rate,
            Filter::HighShelf {
                f: 3500.0,
                db: presence_db,
            },
        );
        self.depth.set(
            self.rate,
            Filter::Peak {
                f: 90.0,
                q: 0.8,
                db: depth_db,
            },
        );
        self.xf_lp.set(self.rate, Filter::LowPass { f: xf_lp, q: 0.7 });
    }

    /// The back panel (`v[front..]`: sag, bias, power tubes, rectifier, power master), on top of the voice's power
    /// stage; `stock` is the amp's own power tube (its voicing is already in the voice).
    fn back_panel(&mut self, v: &[f32; MAX_KNOBS], front: usize, stock: usize) {
        let (sag, bias, tubes, rect, pm) = (
            unit10(v[front]),
            unit10(v[front + 1]),
            v[front + 2].round().clamp(0.0, 4.0) as usize,
            v[front + 3] > 0.5,
            unit10(v[front + 4]),
        );
        // the supply: a tube rectifier droops further and recovers slower (spongy); silicon is firm
        let solid = rect;
        self.sag = 0.9 * sag * if solid { 0.6 } else { 1.0 };
        self.sag_att = 1.0 - (-1.0 / (self.rate * if solid { 0.008 } else { 0.012 })).exp();
        self.sag_rel = 1.0 - (-1.0 / (self.rate * if solid { 0.12 } else { 0.25 })).exp();
        // the tubes: headroom, curve and EQ relative to the amp's own
        let mut drive = self.drive.target;
        if tubes != stock {
            let (t, s) = (tube(tubes), tube(stock));
            drive *= t.drive / s.drive;
            self.pow_sh.set_curve(t.curve);
            self.tube_eq[0].set(self.rate, t.eq);
            self.tube_eq[1].set(self.rate, inverse(s.eq));
            self.ntube = 2;
        } else {
            self.ntube = 0;
        }
        // bias: below noon a crossover notch (cold), above it earlier, lopsided clipping (hot)
        let cold = ((0.5 - bias) * 2.0).max(0.0);
        let hot = ((bias - 0.5) * 2.0).max(0.0);
        self.xover = 0.7 * cold;
        drive *= 1.0 + 0.5 * hot;
        self.pow_bias = 0.25 * hot;
        self.pow_bias_f = self.pow_sh.curve.f(self.pow_bias as f64) as f32;
        // power master: how hard the power tubes are driven, -12..+12 dB about noon
        drive *= db_to_gain((pm - 0.5) * 24.0);
        self.drive.target = drive;
    }
}

impl Pedal for Amp {
    fn process(&mut self, x: &mut [f32]) {
        let Amp {
            input_hp,
            pre,
            npre,
            stages,
            nstages,
            stack_at,
            ts,
            ts_gain,
            os_pre,
            post,
            npost,
            geq,
            ngeq,
            presence,
            depth,
            drive,
            sag,
            sag_env,
            sag_att,
            sag_rel,
            os_pow,
            pow_sh,
            tube_eq,
            ntube,
            xover,
            xover_w,
            pow_bias,
            pow_bias_f,
            cut,
            xf_hp,
            xf_lp,
            level,
            ..
        } = self;
        let (n, at, tg) = (*nstages, *stack_at, *ts_gain);
        let (xo, xw, pb, pbf) = (*xover, *xover_w, *pow_bias, *pow_bias_f);
        for v in x.iter_mut() {
            let mut s = input_hp.hp(*v);
            for b in pre.iter_mut().take(*npre) {
                s = b.tick(s);
            }
            // the preamp, at twice the rate
            s = os_pre.run(s, |u| {
                let mut t = u;
                for (i, st) in stages.iter_mut().take(n).enumerate() {
                    t = st.tick(t);
                    if i == 0 && at == StackAt::AfterFirst {
                        t = ts.tick(t) * tg;
                    }
                }
                t
            });
            if at == StackAt::AfterPreamp {
                s = ts.tick(s) * tg;
            }
            for b in post.iter_mut().take(*npost) {
                s = b.tick(s);
            }
            for b in geq.iter_mut().take(*ngeq) {
                s = b.tick(s);
            }
            // the power stage
            s = depth.tick(presence.tick(s));
            for b in tube_eq.iter_mut().take(*ntube) {
                s = b.tick(s);
            }
            // supply sag: while the tubes work hard the supply droops, and with it the ceiling they clip at: the
            // level dips and the stage squashes sooner, then blooms back as the supply recovers
            let d = drive.tick();
            let c = 1.0 / (1.0 + *sag * *sag_env);
            let ic = 1.0 / c;
            let (sh, ct) = (&mut *pow_sh, &mut *cut);
            let y = os_pow.run(s * d, |u| {
                // a cold-biased push-pull pair: both tubes nearly off around zero (crossover distortion)
                let u = if xo > 0.0 {
                    u - xo * xw * (u / xw).tanh()
                } else {
                    u
                };
                ct.lp(c * (sh.tick(u * ic + pb) - pbf))
            });
            let a = y.abs();
            *sag_env = flush(*sag_env + (a - *sag_env) * if a > *sag_env { *sag_att } else { *sag_rel });
            let out = xf_lp.tick(xf_hp.hp(y)) / d.max(0.2).sqrt();
            *v = out * level.tick();
        }
        if let Some(e) = self.extra.as_mut() {
            e.run(x);
        }
    }

    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        let rate2 = self.rate * 2.0;
        // the input coupling cap and the output transformer: a bass amp's reach a 5-string's low B (P37.2)
        let bass_voice = matches!(
            self.voice,
            Voice::Bass | Voice::B15 | Voice::BassSs | Voice::BassTube
        );
        self.input_hp.set(self.rate, if bass_voice { 5.0 } else { 25.0 });
        self.ngeq = 0;
        // (P41.2, D166: the Rectifier, the 800 and the tweed's transformers reach lower)
        let xf = match self.voice {
            Voice::Modern | Voice::Lead => 30.0,
            Voice::Tweed => 25.0,
            _ => 55.0,
        };
        self.xf_hp.set(self.rate, if bass_voice { 6.0 } else { xf });
        match self.voice {
            Voice::Clean => {
                // bright, volume ("gain"), treble, middle, bass, reverb, speed, intensity, master (P41.2: the real
                // panel's order)
                let (bright, g, treble, mid, bass, master) = (
                    v[0] > 0.5,
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                    unit10(v[8]),
                );
                self.pre(&[Filter::HighShelf {
                    f: 2500.0,
                    db: if bright { 6.0 * (1.0 - 0.7 * g) } else { 0.0 },
                }]);
                self.nstages = 2;
                self.stages[0].tune(rate2, 2.2, Curve::Asym(0.75), 0.12, 13000.0, 20.0);
                // the volume pot sits between the tone stack and the second stage
                self.stages[1].tune(
                    rate2,
                    0.6 + 26.0 * audio_taper(g),
                    Curve::Asym(0.8),
                    0.1,
                    11000.0,
                    25.0,
                );
                self.stages[0].cathode(rate2, 120.0, 0.7);
                self.stages[1].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterFirst, TWIN, treble, mid, bass, 6.0);
                self.npost = 0;
                self.power(Curve::Tanh, 0.35 + 1.8 * audio_taper(master), 1.0, 1.5, 9500.0);
                self.cut.set(rate2, 18000.0);
                // (P37.2: past noon the Twin's volume got 12 dB louder from 3 to 7 and into the board's limiter;
                // a steeper trim above 5 keeps it rising, as a turned-up amp does, by about half that)
                self.level.target = db_to_gain(-1.0 - 4.0 * g - 20.0 * (g - 0.5).max(0.0));
                let rate = self.rate;
                if let Some(e) = self.extra.as_mut() {
                    e.set(rate, unit10(v[5]), unit10(v[6]), unit10(v[7]));
                }
            }
            Voice::Tweed => {
                let (vol, tone, master) = (unit10(v[0]), unit10(v[1]), unit10(v[2]));
                // (P41.2, D166: the input and coupling caps let the lows through, and the depth grows as it's turned up and
                // the supply loosens: 15-20% of a chug under 100 Hz, as the 5E3 and Bassman captures; it was 8%)
                self.pre(&[Filter::HighPass { f: 25.0, q: 0.6 }]);
                self.nstages = 2;
                self.stages[0].tune(rate2, 3.0, Curve::Asym(0.7), 0.15, 11000.0, 25.0);
                self.stages[1].tune(
                    rate2,
                    1.0 + 30.0 * audio_taper(vol),
                    Curve::Asym(0.6),
                    0.2,
                    8000.0,
                    20.0,
                );
                self.stages[0].cathode(rate2, 100.0, 1.0);
                self.stages[1].cathode(rate2, 100.0, 1.0);
                self.stack_at = StackAt::None;
                // the treble-bleed tone knob: darker to brighter
                self.post(&[
                    Filter::LowPass {
                        f: sweep(1800.0, 9000.0, tone),
                        q: 0.6,
                    },
                    Filter::HighShelf {
                        f: 2000.0,
                        db: -3.0 + 6.0 * tone,
                    },
                    Filter::Peak {
                        f: 700.0,
                        q: 0.8,
                        db: 2.0,
                    },
                ]);
                // 6V6s with no negative feedback: early, loose, saggy breakup
                self.power(
                    Curve::Asym(0.85),
                    0.7 + 3.5 * audio_taper(master),
                    0.0,
                    6.0 + 4.0 * vol,
                    6500.0,
                );
                self.cut.set(rate2, 18000.0);
                self.level.target = db_to_gain(-13.0 - 5.0 * vol);
            }
            Voice::Chime => {
                let (g, treble, bass, cutk, master) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                );
                self.pre(&[Filter::HighShelf { f: 3000.0, db: 3.0 }]);
                self.nstages = 2;
                self.stages[0].tune(
                    rate2,
                    3.0 + 10.0 * audio_taper(g),
                    Curve::Asym(0.8),
                    0.1,
                    14000.0,
                    30.0,
                );
                self.stages[1].tune(rate2, 1.5 + 9.0 * g, Curve::Asym(0.8), 0.1, 12000.0, 40.0);
                self.stages[0].cathode(rate2, 150.0, 0.6);
                self.stages[1].cathode(rate2, 150.0, 0.8);
                self.stack(StackAt::AfterFirst, AC30, treble, 0.5, bass, 8.0);
                self.npost = 0;
                // EL84s in class A: lopsided (even harmonics), the Cut control a low-pass in the phase inverter
                self.power(
                    Curve::Asym(0.75),
                    0.6 + 3.0 * audio_taper(master),
                    2.0,
                    1.0,
                    10000.0,
                );
                self.cut.set(rate2, sweep(14000.0, 2200.0, cutk));
                self.level.target = db_to_gain(-13.0 - 5.0 * g);
            }
            Voice::Crunch => {
                let (g, bass, mid, treble, pres, master) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                    unit10(v[5]),
                );
                // the bright cap on the volume pot: more top at low settings
                self.pre(&[Filter::HighShelf {
                    f: 2000.0,
                    db: 5.0 * (1.0 - 0.6 * g),
                }]);
                self.nstages = 3;
                self.stages[0].tune(
                    rate2,
                    2.5 + 22.0 * audio_taper(g),
                    Curve::Asym(0.7),
                    0.15,
                    10000.0,
                    30.0,
                );
                self.stages[1].tune(rate2, 6.0, Curve::Asym(0.8), 0.1, 8000.0, 60.0);
                // the cathode follower: a soft limit on the way into the stack
                self.stages[2].tune(rate2, 1.0, Curve::Soft, 0.0, 16000.0, 10.0);
                self.stages[0].cathode(rate2, 160.0, 0.45);
                self.stages[1].cathode(rate2, 200.0, 0.5);
                self.stages[2].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterPreamp, JTM45, treble, mid, bass, 9.0);
                self.npost = 0;
                self.power(
                    Curve::Tanh,
                    0.5 + 3.0 * audio_taper(master),
                    8.0 * pres,
                    2.0,
                    8500.0,
                );
                self.cut.set(rate2, 18000.0);
                self.level.target = db_to_gain(-15.0 - 3.0 * g);
            }
            Voice::Lead => {
                let (g, bass, mid, treble, pres, master) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                    unit10(v[5]),
                );
                self.pre(&[
                    Filter::HighPass { f: 50.0, q: 0.6 },
                    Filter::HighShelf { f: 2500.0, db: 4.0 },
                ]);
                self.nstages = 3;
                self.stages[0].tune(
                    rate2,
                    4.0 + 28.0 * audio_taper(g),
                    Curve::Asym(0.7),
                    0.15,
                    9000.0,
                    50.0,
                );
                // the cold-biased second stage: tight, lopsided
                // (P41.2, D166: its coupling cap 120 Hz to 70 and the input's 70 to 50: the JCM800 captures' 3-6% under
                // 100 Hz, not 1%)
                self.stages[1].tune(rate2, 12.0, Curve::Asym(0.75), 0.1, 7000.0, 70.0);
                self.stages[2].tune(rate2, 1.0, Curve::Soft, 0.0, 16000.0, 10.0);
                self.stages[0].cathode(rate2, 160.0, 0.4);
                // the 2203's second stage has an unbypassed cathode: much less gain down low
                self.stages[1].cathode(rate2, 250.0, 0.3);
                self.stages[2].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterPreamp, JCM800, treble, mid, bass, 9.0);
                self.post(&[Filter::LowShelf { f: 110.0, db: 6.0 }]);
                self.power(
                    Curve::Tanh,
                    0.5 + 3.0 * audio_taper(master),
                    8.0 * pres,
                    2.0,
                    8500.0,
                );
                self.cut.set(rate2, 18000.0);
                self.level.target = db_to_gain(-15.0 - 2.0 * g);
            }
            Voice::Modern => {
                let (g, bass, mid, treble, pres, dep, master) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                    unit10(v[5]),
                    unit10(v[6]),
                );
                // the channel's mode (P37.6c): Raw (the least gain, open and gritty), Vintage (smoother, more
                // mids, a softer low end) or Modern (as it always was)
                let (g1, g2, push, dep) = match v[7].round() as i32 {
                    0 => (0.35, 6.0, 0.0, dep * 0.4),
                    1 => (0.7, 12.0, 3.5, dep * 0.5),
                    _ => (1.0, 18.0, 2.0, dep),
                };
                // (P41.2, D166: turned down, the later stages give up their gain too, so a Rectifier at 3 is a
                // crunch with a quiet idle, not the full cascade's +48 dB of hiss; from 6 up it is as it was)
                let s = ((g - 0.1) / 0.5).clamp(0.0, 1.0);
                // a firm front end: the lows trimmed before the gain (less than before: the real one's chug is
                // loose), the mids pushed
                self.pre(&[
                    Filter::HighPass { f: 90.0, q: 0.7 },
                    Filter::Peak {
                        f: 900.0,
                        q: 0.7,
                        db: push,
                    },
                ]);
                self.nstages = 4;
                self.stages[0].tune(
                    rate2,
                    (2.0 + 38.0 * audio_taper(g)) * g1,
                    Curve::Asym(0.7),
                    0.15,
                    7000.0,
                    30.0,
                );
                self.stages[1].tune(rate2, g2 * (0.2 + 0.8 * s), Curve::Asym(0.75), 0.1, 5500.0, 90.0);
                self.stages[2].tune(
                    rate2,
                    9.0 * (0.12 + 0.88 * s),
                    Curve::Asym(0.8),
                    0.1,
                    5000.0,
                    70.0,
                );
                self.stages[3].tune(rate2, 1.0, Curve::Soft, 0.0, 16000.0, 10.0);
                self.stages[0].cathode(rate2, 180.0, 0.5);
                self.stages[1].cathode(rate2, 220.0, 0.45);
                self.stages[2].cathode(rate2, 150.0, 0.6);
                self.stages[3].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterPreamp, MESA, treble, mid, bass, 10.0);
                // (P41.2, D166: the 6L6s into a big transformer: the lows the preamp's clipping makes come through,
                // 10-15% of a chug's energy under 100 Hz as in the owner's Dual Rec captures; it was 2%)
                self.post(&[Filter::LowShelf { f: 110.0, db: 11.0 }]);
                self.power(
                    Curve::Tanh,
                    0.6 + 3.0 * audio_taper(master),
                    8.0 * pres,
                    10.0 * dep,
                    8000.0,
                );
                self.cut.set(rate2, 18000.0);
                self.level.target = db_to_gain(-10.5);
            }
            Voice::Bass => {
                let (g, bass, mid, freq, treble, master) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    v[3],
                    unit10(v[4]),
                    unit10(v[5]),
                );
                let (ultra_lo, ultra_hi) = (v[6] >= 0.5, v[7] >= 0.5);
                // Ultra Hi: the SVT's treble lift at the input; (P37.2: the high-pass 25 Hz to 6)
                self.pre(&[
                    Filter::HighPass { f: 6.0, q: 0.5 },
                    Filter::HighShelf { f: 4500.0, db: 8.0 },
                ]);
                if !ultra_hi {
                    self.npre = 1;
                }
                self.nstages = 2;
                self.stages[0].tune(
                    rate2,
                    2.0 + 9.0 * audio_taper(g),
                    Curve::Asym(0.8),
                    0.1,
                    12000.0,
                    5.0,
                );
                self.stages[1].tune(rate2, 1.0 + 6.0 * g, Curve::Asym(0.85), 0.08, 10000.0, 6.0);
                // the SVT's bass and treble; its mid is a sweepable boost or cut
                self.stages[0].cathode(rate2, 100.0, 1.0);
                self.stages[1].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterFirst, AMPEG, treble, 0.5, bass, 6.0);
                // Ultra Lo: a deep mid scoop and a little more bottom
                self.post(&[
                    Filter::Peak {
                        f: freq.clamp(100.0, 4000.0),
                        q: 0.9,
                        db: (mid - 0.5) * 20.0,
                    },
                    Filter::Peak {
                        f: 500.0,
                        q: 0.7,
                        db: -10.0,
                    },
                    Filter::LowShelf { f: 50.0, db: 3.0 },
                ]);
                if !ultra_lo {
                    self.npost = 1;
                }
                self.power(Curve::Tanh, 0.35 + 2.0 * audio_taper(master), 0.0, 2.0, 7000.0);
                self.cut.set(rate2, 18000.0);
                self.level.target = db_to_gain(-12.0 - 3.0 * g);
            }
            Voice::B15 => {
                let (vol, bass, treble, hi_in, master) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    v[3] >= 0.5,
                    unit10(v[4]),
                );
                self.pre(&[Filter::HighPass { f: 6.0, q: 0.5 }]);
                self.nstages = 2;
                // the Hi input is the one most players use: about 6 dB more into the first triode
                self.stages[0].tune(
                    rate2,
                    if hi_in { 3.0 } else { 1.5 },
                    Curve::Asym(0.8),
                    0.1,
                    11000.0,
                    5.0,
                );
                self.stages[1].tune(
                    rate2,
                    1.0 + 14.0 * audio_taper(vol),
                    Curve::Asym(0.7),
                    0.15,
                    6500.0,
                    5.0,
                );
                self.stages[0].cathode(rate2, 100.0, 1.0);
                self.stages[1].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterFirst, BASSMAN, treble, 0.5, bass, 6.0);
                self.npost = 0;
                // two 6V6s and a small output transformer: early, soft, round
                self.power(
                    Curve::Asym(0.85),
                    0.7 + 2.5 * audio_taper(master),
                    0.0,
                    2.0,
                    5000.0,
                );
                self.cut.set(rate2, 16000.0);
                self.level.target = db_to_gain(-13.0 - 4.0 * vol);
            }
            Voice::BassSs => {
                let (g, bass_db, mid_db, freq, treble_db, contour, master) =
                    (unit10(v[0]), v[1], v[2], v[3], v[4], v[5] >= 0.5, unit10(v[6]));
                // the Contour: a wide scoop around 500 Hz before the preamp (slap)
                self.pre(&[
                    Filter::HighPass { f: 6.0, q: 0.5 },
                    Filter::Peak {
                        f: 500.0,
                        q: 0.8,
                        db: -10.0,
                    },
                ]);
                if !contour {
                    self.npre = 1;
                }
                self.nstages = 2;
                // a clean op-amp preamp, then the boost's hard clipping as Gain goes up
                self.stages[0].tune(rate2, 1.2 + 6.0 * audio_taper(g), Curve::Soft, 0.0, 16000.0, 5.0);
                self.stages[1].tune(rate2, 0.8 + 6.0 * g * g, Curve::Hard, 0.0, 12000.0, 6.0);
                self.stages[0].cathode(rate2, 100.0, 1.0);
                self.stages[1].cathode(rate2, 100.0, 1.0);
                self.stack_at = StackAt::None;
                // the active EQ: shelves at 60 Hz and 5 kHz, a semi-parametric mid
                self.post(&[
                    Filter::LowShelf { f: 60.0, db: bass_db },
                    Filter::Peak {
                        f: freq.clamp(100.0, 4000.0),
                        q: 0.8,
                        db: mid_db,
                    },
                    Filter::HighShelf {
                        f: 5000.0,
                        db: treble_db,
                    },
                ]);
                // a transformerless power stage: wide and flat, a little clipping at the top
                self.power(Curve::Tanh, 0.25 + 1.2 * audio_taper(master), 0.0, 0.0, 14000.0);
                self.cut.set(rate2, 18000.0);
                // (the boost's hard clipping adds a lot of level as Gain goes up: taken back)
                self.level.target = db_to_gain(0.5 - 4.0 * g - 14.0 * g * g);
            }
            Voice::BassTube => {
                let (g, bass, mid, treble, master) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                );
                self.pre(&[Filter::HighPass { f: 6.0, q: 0.5 }]);
                self.nstages = 3;
                self.stages[0].tune(
                    rate2,
                    1.2 + 18.0 * audio_taper(g),
                    Curve::Asym(0.7),
                    0.15,
                    10000.0,
                    5.0,
                );
                self.stages[1].tune(rate2, 1.5 + 3.0 * g, Curve::Asym(0.8), 0.1, 6500.0, 6.0);
                self.stages[2].tune(rate2, 1.0, Curve::Soft, 0.0, 16000.0, 10.0);
                self.stages[0].cathode(rate2, 100.0, 1.0);
                self.stages[1].cathode(rate2, 120.0, 0.8);
                self.stages[2].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterPreamp, BASSMAN, treble, mid, bass, 9.0);
                self.npost = 0;
                self.power(Curve::Tanh, 0.5 + 3.0 * audio_taper(master), 2.0, 3.0, 6500.0);
                self.cut.set(rate2, 18000.0);
                self.level.target = db_to_gain(-13.0 - 3.0 * g);
            }
            Voice::Cali => {
                // the Mark IIC+'s lead channel: Gain (its Lead Drive), Treble, Bass, Middle, Master, Presence, then
                // the graphic EQ's five sliders (dB)
                let (g, treble, bass, mid, master, pres) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                    unit10(v[5]),
                );
                self.pre(&[
                    Filter::HighPass { f: 60.0, q: 0.7 },
                    Filter::HighShelf { f: 2000.0, db: 2.0 },
                ]);
                self.nstages = 4;
                // the tone stack straight after the first triode, before the cascade: what the cascade clips is
                // already shaped, so the Bass knob tightens or loosens the distortion itself (the Mark's tight,
                // mid-heavy push); then the lead drive and two more stages
                self.stages[0].tune(rate2, 3.0, Curve::Asym(0.75), 0.12, 12000.0, 30.0);
                self.stages[1].tune(
                    rate2,
                    2.0 + 40.0 * audio_taper(g),
                    Curve::Asym(0.7),
                    0.15,
                    6500.0,
                    80.0,
                );
                self.stages[2].tune(rate2, 10.0, Curve::Asym(0.8), 0.1, 6000.0, 60.0);
                self.stages[3].tune(rate2, 1.0, Curve::Soft, 0.0, 16000.0, 10.0);
                self.stages[0].cathode(rate2, 150.0, 0.7);
                self.stages[1].cathode(rate2, 250.0, 0.35);
                self.stages[2].cathode(rate2, 200.0, 0.5);
                self.stages[3].cathode(rate2, 100.0, 1.0);
                // (the bass pot reads high: the Mark's bass is famously strong in front of the gain)
                self.stack(StackAt::AfterFirst, MESA, treble, mid, bass * 0.7, 12.0);
                // the lead channel's mid hump; the 6L6s' firm low end under it (the owner's Mark captures: 3-8% of a
                // chug under 100 Hz)
                self.post(&[
                    Filter::Peak {
                        f: 700.0,
                        q: 0.8,
                        db: 3.0,
                    },
                    Filter::LowShelf { f: 120.0, db: 8.0 },
                ]);
                for (k, (b, f)) in self.geq.iter_mut().zip(GEQ_HZ).enumerate() {
                    b.set(
                        self.rate,
                        Filter::Peak {
                            f,
                            q: 1.1,
                            db: v[6 + k].clamp(-12.0, 12.0),
                        },
                    );
                }
                self.ngeq = 5;
                // simul-class 6L6s: firm and focused
                self.power(
                    Curve::Tanh,
                    0.5 + 2.5 * audio_taper(master),
                    7.0 * pres,
                    1.0,
                    8000.0,
                );
                self.cut.set(rate2, 18000.0);
                self.level.target = db_to_gain(-14.0);
            }
            Voice::Snake => {
                // the SLO / 5150 overdrive channel: a Marshall-style cascade hot-rodded to four stages, smoother on
                // top (lower Miller corners) and with more gain than the 800
                let (g, bass, mid, treble, master, pres) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                    unit10(v[5]),
                );
                self.pre(&[
                    Filter::HighPass { f: 55.0, q: 0.6 },
                    Filter::HighShelf { f: 2200.0, db: 3.0 },
                ]);
                self.nstages = 4;
                self.stages[0].tune(
                    rate2,
                    3.0 + 34.0 * audio_taper(g),
                    Curve::Asym(0.7),
                    0.15,
                    8000.0,
                    40.0,
                );
                self.stages[1].tune(rate2, 14.0, Curve::Asym(0.75), 0.1, 6000.0, 80.0);
                self.stages[2].tune(rate2, 7.0, Curve::Asym(0.8), 0.1, 5000.0, 60.0);
                self.stages[3].tune(rate2, 1.0, Curve::Soft, 0.0, 16000.0, 10.0);
                self.stages[0].cathode(rate2, 160.0, 0.45);
                self.stages[1].cathode(rate2, 250.0, 0.35);
                self.stages[2].cathode(rate2, 180.0, 0.6);
                self.stages[3].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterPreamp, JCM800, treble, mid, bass, 10.0);
                // the lead's vocal upper mid, a firm low end from the 6L6s
                self.post(&[
                    Filter::Peak {
                        f: 1100.0,
                        q: 0.9,
                        db: 2.5,
                    },
                    Filter::LowShelf { f: 110.0, db: 5.0 },
                ]);
                self.power(
                    Curve::Tanh,
                    0.55 + 3.0 * audio_taper(master),
                    8.0 * pres,
                    3.0,
                    7500.0,
                );
                self.cut.set(rate2, 16000.0);
                self.level.target = db_to_gain(-16.5);
            }
            Voice::Tangerine => {
                // the Orange dirty channel as its panel reads: Volume (the master), Treble, Middle, Bass, Gain
                let (master, treble, mid, bass, g) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                );
                self.pre(&[Filter::HighPass { f: 50.0, q: 0.6 }]);
                self.nstages = 3;
                self.stages[0].tune(
                    rate2,
                    2.5 + 30.0 * audio_taper(g),
                    Curve::Asym(0.65),
                    0.18,
                    7000.0,
                    35.0,
                );
                self.stages[1].tune(rate2, 9.0, Curve::Asym(0.7), 0.15, 5000.0, 60.0);
                self.stages[2].tune(rate2, 1.0, Curve::Soft, 0.0, 14000.0, 10.0);
                self.stages[0].cathode(rate2, 140.0, 0.55);
                self.stages[1].cathode(rate2, 200.0, 0.5);
                self.stages[2].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterPreamp, JCM800, treble, mid, bass, 10.0);
                // thick low mids, a darker, furrier top than a Marshall: its fuzzy, compressed crunch
                self.post(&[
                    Filter::Peak {
                        f: 450.0,
                        q: 0.8,
                        db: 4.0,
                    },
                    Filter::HighShelf { f: 3500.0, db: -3.0 },
                    Filter::LowShelf { f: 110.0, db: 2.0 },
                ]);
                self.power(Curve::Tanh, 0.7 + 3.0 * audio_taper(master), 0.0, 2.0, 7000.0);
                self.cut.set(rate2, 15000.0);
                self.level.target = db_to_gain(-15.5 - 2.0 * g);
            }
            Voice::Surrey => {
                // the DR103's panel: Normal and Brilliant volumes (both channels jumpered, as most players use it),
                // Bass, Treble, Middle, Presence, Master
                let (normal, brilliant, bass, treble, mid, pres, master) = (
                    unit10(v[0]),
                    unit10(v[1]),
                    unit10(v[2]),
                    unit10(v[3]),
                    unit10(v[4]),
                    unit10(v[5]),
                    unit10(v[6]),
                );
                let vol = (normal + brilliant).min(1.4);
                let bright = brilliant / (normal + brilliant).max(0.05);
                // the Brilliant channel's bright cap: more top as it takes over, less as the volume goes up
                self.pre(&[
                    Filter::HighPass { f: 40.0, q: 0.6 },
                    Filter::HighShelf {
                        f: 2500.0,
                        db: 7.0 * bright * (1.0 - 0.4 * vol.min(1.0)),
                    },
                ]);
                self.nstages = 3;
                // a low-gain, high-headroom preamp: clean far up, then a firm, punchy breakup
                self.stages[0].tune(rate2, 2.0, Curve::Asym(0.8), 0.1, 14000.0, 25.0);
                self.stages[1].tune(
                    rate2,
                    0.5 + 9.0 * audio_taper(vol / 1.4),
                    Curve::Asym(0.8),
                    0.1,
                    11000.0,
                    30.0,
                );
                self.stages[2].tune(rate2, 1.0, Curve::Soft, 0.0, 16000.0, 10.0);
                self.stages[0].cathode(rate2, 120.0, 0.8);
                self.stages[1].cathode(rate2, 150.0, 0.8);
                self.stages[2].cathode(rate2, 100.0, 1.0);
                self.stack(StackAt::AfterPreamp, JTM45, treble, mid, bass, 9.0);
                // the mids forward and even: it cuts without fizz
                self.post(&[Filter::Peak {
                    f: 800.0,
                    q: 0.6,
                    db: 2.0,
                }]);
                // four EL34s into a big transformer: lots of clean headroom
                self.power(
                    Curve::Tanh,
                    0.3 + 1.6 * audio_taper(master),
                    7.0 * pres,
                    2.0,
                    9500.0,
                );
                self.cut.set(rate2, 18000.0);
                self.level.target = db_to_gain(-5.0 - 5.0 * vol);
            }
        }
        // the back panel: (front knobs, the amp's own power tube in `catalog::TUBES`)
        let (front, stock) = match self.voice {
            Voice::Clean => (9, 1),
            Voice::Tweed => (3, 4),
            Voice::Chime => (5, 3),
            Voice::Crunch | Voice::Lead => (6, 0),
            Voice::Modern => (8, 1),
            Voice::Bass => (8, 2),
            Voice::B15 => (5, 4),
            Voice::BassSs => (7, 1),
            Voice::BassTube => (5, 0),
            Voice::Cali => (11, 1),
            Voice::Snake => (6, 1),
            Voice::Tangerine => (5, 0),
            Voice::Surrey => (7, 0),
        };
        self.back_panel(v, front, stock);
    }

    fn reset(&mut self) {
        self.input_hp.reset();
        for b in self
            .pre
            .iter_mut()
            .chain(self.post.iter_mut())
            .chain(self.geq.iter_mut())
        {
            b.reset();
        }
        for s in &mut self.stages {
            s.reset();
        }
        self.ts.reset();
        self.os_pre.reset();
        self.os_pow.reset();
        self.presence.reset();
        self.depth.reset();
        self.pow_sh.reset_at(self.pow_bias);
        for b in &mut self.tube_eq {
            b.reset();
        }
        self.cut.reset();
        self.xf_hp.reset();
        self.xf_lp.reset();
        self.sag_env = 0.0;
        self.drive.snap();
        self.level.snap();
        if let Some(e) = self.extra.as_mut() {
            e.spring.reset();
            e.trem.phase = 0.0;
            e.trem_depth.snap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tone_stack_scoops_the_mids_and_its_knobs_do_what_they_say() {
        // the Bassman at noon: the classic mid scoop, a few dB down around 400-700 Hz against 100 Hz and 3 kHz
        let at = |f: f64, t: f64, m: f64, b: f64| BASSMAN.response_db(f, t, m, b);
        let (lo, mid, hi) = (
            at(100.0, 0.5, 0.5, 0.5),
            at(500.0, 0.5, 0.5, 0.5),
            at(3000.0, 0.5, 0.5, 0.5),
        );
        assert!(mid < lo - 3.0 && mid < hi - 3.0, "{lo} {mid} {hi}");
        assert!(
            at(5000.0, 1.0, 0.5, 0.5) > at(5000.0, 0.0, 0.5, 0.5) + 6.0,
            "treble"
        );
        assert!(at(80.0, 0.5, 0.5, 1.0) > at(80.0, 0.5, 0.5, 0.0) + 6.0, "bass");
        assert!(at(600.0, 0.5, 1.0, 0.5) > at(600.0, 0.5, 0.0, 0.5) + 6.0, "mid");
    }

    #[test]
    fn the_digital_tone_stack_matches_the_analog_one() {
        let rate = 96000.0;
        for stack in [TWIN, BASSMAN, JCM800, MESA, AC30, AMPEG, JTM45] {
            let mut ts = ToneStack::default();
            ts.set(rate, &stack, 0.6, 0.4, 0.7);
            for f in [100.0f64, 1000.0, 4000.0] {
                let x: Vec<f32> = (0..96000)
                    .map(|i| (std::f64::consts::TAU * f * i as f64 / rate).sin() as f32)
                    .collect();
                ts.reset();
                let y: Vec<f32> = x.iter().map(|&s| ts.tick(s)).collect();
                let rms = (y[48000..].iter().map(|v| (v * v) as f64).sum::<f64>() / 48000.0).sqrt();
                let db = 20.0 * (rms / std::f64::consts::FRAC_1_SQRT_2).log10();
                let want = stack.response_db(f, 0.6, 0.4, 0.7);
                assert!((db - want).abs() < 0.3, "{f} Hz: {db} vs {want}");
            }
        }
    }
}
