//! DSP parts for the board's models, on top of `tone::dsp`: values that glide, topology-preserving one-pole and
//! state-variable filters (Zavalishin), 2x oversampling by polyphase IIR half-band filters (Valenzuela and
//! Constantinides' design, as Laurent de Soras's HIIR computes it), waveshapers with first-order antiderivative
//! antialiasing (Parker, Zavalishin and Le Bivic, DAFx-16), LFOs, power-of-two delay lines with cubic reads, a
//! soft limiter. Everything allocates in `new` and never in `tick`/`process`.

use std::f32::consts::{PI, TAU};

pub use crate::tone::dsp::{Biquad, Envelope, Filter, db_to_gain};

/// Keep denormals out of feedback paths.
#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// Per-sample coefficient of a one-pole smoother with time constant `ms`.
pub fn coef_ms(rate: f32, ms: f32) -> f32 {
    1.0 - (-1.0 / (rate * ms.max(0.01) * 1e-3)).exp()
}

/// A value that glides to its target (one pole), so knob moves never click.
#[derive(Clone, Copy, Debug)]
pub struct Smooth {
    pub value: f32,
    pub target: f32,
    k: f32,
}

impl Smooth {
    pub fn new(rate: f32, ms: f32, v: f32) -> Smooth {
        Smooth {
            value: v,
            target: v,
            k: coef_ms(rate, ms),
        }
    }
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.k;
        self.value
    }
    /// Advance `n` samples at once (for values read once a sub-block).
    pub fn skip(&mut self, n: usize) -> f32 {
        let k = 1.0 - (1.0 - self.k).powi(n as i32);
        self.value += (self.target - self.value) * k;
        self.value
    }
    pub fn snap(&mut self) {
        self.value = self.target;
    }
}

/// Topology-preserving one-pole filter (bilinear, cutoff-exact): low-pass, high-pass, and the all-pass built
/// from them.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    g: f32,
    s: f32,
}

impl OnePole {
    pub fn new(rate: f32, f: f32) -> OnePole {
        let mut p = OnePole::default();
        p.set(rate, f);
        p
    }
    pub fn set(&mut self, rate: f32, f: f32) {
        let wc = (PI * (f / rate).clamp(1e-6, 0.49)).tan();
        self.g = wc / (1.0 + wc);
    }
    #[inline]
    pub fn lp(&mut self, x: f32) -> f32 {
        let v = (x - self.s) * self.g;
        let y = v + self.s;
        self.s = flush(y + v);
        y
    }
    #[inline]
    pub fn hp(&mut self, x: f32) -> f32 {
        x - self.lp(x)
    }
    /// First-order all-pass (phase from 0 at DC to -180 degrees at Nyquist, -90 at the cutoff).
    #[inline]
    pub fn ap(&mut self, x: f32) -> f32 {
        let l = self.lp(x);
        2.0 * l - x
    }
    pub fn reset(&mut self) {
        self.s = 0.0;
    }
}

/// The state-variable filter in its trapezoidal form (Simper's): stable however fast its cutoff moves, which is
/// what a wah or an envelope filter needs.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic1: f32,
    ic2: f32,
}

/// One SVF step's outputs.
#[derive(Clone, Copy, Debug)]
pub struct SvfOut {
    pub lp: f32,
    pub bp: f32,
    pub hp: f32,
}

impl Svf {
    pub fn new(rate: f32, f: f32, q: f32) -> Svf {
        let mut s = Svf::default();
        s.set(rate, f, q);
        s
    }
    pub fn set(&mut self, rate: f32, f: f32, q: f32) {
        let g = (PI * (f / rate).clamp(1e-5, 0.49)).tan();
        let k = 1.0 / q.max(0.05);
        self.k = k;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> SvfOut {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        SvfOut {
            lp: v2,
            bp: v1,
            hp: x - self.k * v1 - v2,
        }
    }
    /// The band-pass normalised to unity gain at the centre.
    #[inline]
    pub fn bp_norm(&mut self, x: f32) -> f32 {
        let k = self.k;
        self.tick(x).bp * k
    }
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

// -- 2x oversampling ------------------------------------------------------------------------------------------------

/// The all-pass coefficients of a polyphase IIR half-band filter with `n` coefficients and transition band `tbw`
/// (of the oversampled rate's half band, 0..0.5): Valenzuela and Constantinides' elliptic design as HIIR's
/// `PolyphaseIir2Designer::compute_coefs_spec_order_tbw` computes it.
pub fn halfband_coefs(n: usize, tbw: f64) -> Vec<f64> {
    use std::f64::consts::PI as PI64;
    let mut k = ((1.0 - tbw * 2.0) * PI64 / 4.0).tan();
    k *= k;
    let kksqrt = (1.0 - k * k).powf(0.25);
    let e = 0.5 * (1.0 - kksqrt) / (1.0 + kksqrt);
    let e2 = e * e;
    let e4 = e2 * e2;
    let q = e * (1.0 + e4 * (2.0 + e4 * (15.0 + 150.0 * e4)));
    let order = (n * 2 + 1) as f64;
    let acc_num = |c: f64| {
        let (mut i, mut j, mut acc) = (0.0f64, 1.0f64, 0.0f64);
        loop {
            let v = q.powf(i * (i + 1.0)) * ((i * 2.0 + 1.0) * c * PI64 / order).sin() * j;
            acc += v;
            j = -j;
            i += 1.0;
            if v.abs() <= 1e-100 || i > 200.0 {
                break;
            }
        }
        acc
    };
    let acc_den = |c: f64| {
        let (mut i, mut j, mut acc) = (1.0f64, -1.0f64, 0.0f64);
        loop {
            let v = q.powf(i * i) * (i * 2.0 * c * PI64 / order).cos() * j;
            acc += v;
            j = -j;
            i += 1.0;
            if v.abs() <= 1e-100 || i > 200.0 {
                break;
            }
        }
        acc
    };
    (0..n)
        .map(|index| {
            let c = (index + 1) as f64;
            let num = acc_num(c) * q.powf(0.25);
            let den = acc_den(c) + 0.5;
            let ww = num / den;
            let wwsq = ww * ww;
            let x = ((1.0 - wwsq * k) * (1.0 - wwsq / k)).sqrt() / (1.0 + wwsq);
            (1.0 - x) / (1.0 + x)
        })
        .collect()
}

/// Coefficients used by every oversampler: 8 all-passes, transition 0.046 of the high rate (at 48 kHz: flat to
/// ~19.6 kHz), about 96 dB of image and alias rejection.
const HB_N: usize = 8;

fn hb_coefs() -> [f32; HB_N] {
    use std::sync::OnceLock;
    static C: OnceLock<[f32; HB_N]> = OnceLock::new();
    *C.get_or_init(|| {
        let v = halfband_coefs(HB_N, 0.046);
        let mut a = [0.0f32; HB_N];
        for (o, c) in a.iter_mut().zip(v) {
            *o = c as f32;
        }
        a
    })
}

/// One polyphase all-pass chain pair (HIIR's StageProc): even coefficients on path 0, odd on path 1.
#[derive(Clone, Copy, Debug)]
struct Stages {
    c: [f32; HB_N],
    x: [f32; HB_N],
    y: [f32; HB_N],
}

impl Stages {
    fn new() -> Stages {
        Stages {
            c: hb_coefs(),
            x: [0.0; HB_N],
            y: [0.0; HB_N],
        }
    }
    #[inline]
    fn run(&mut self, mut s0: f32, mut s1: f32) -> (f32, f32) {
        let mut i = 0;
        while i < HB_N {
            let t0 = (s0 - self.y[i]) * self.c[i] + self.x[i];
            self.x[i] = s0;
            self.y[i] = flush(t0);
            s0 = t0;
            let t1 = (s1 - self.y[i + 1]) * self.c[i + 1] + self.x[i + 1];
            self.x[i + 1] = s1;
            self.y[i + 1] = flush(t1);
            s1 = t1;
            i += 2;
        }
        (s0, s1)
    }
    fn reset(&mut self) {
        self.x = [0.0; HB_N];
        self.y = [0.0; HB_N];
    }
}

/// 2x up- and down-sampling around a nonlinearity: `run(x, f)` feeds two samples at twice the rate through `f`
/// and returns one at the base rate. Latency is a few samples (minimum phase, no FIR).
#[derive(Clone, Copy, Debug)]
pub struct Oversample2 {
    up: Stages,
    down: Stages,
}

impl Default for Oversample2 {
    fn default() -> Self {
        Oversample2::new()
    }
}

impl Oversample2 {
    pub fn new() -> Oversample2 {
        Oversample2 {
            up: Stages::new(),
            down: Stages::new(),
        }
    }
    /// Upsample one sample to two.
    #[inline]
    pub fn up(&mut self, x: f32) -> (f32, f32) {
        self.up.run(x, x)
    }
    /// Downsample two samples (in time order) to one.
    #[inline]
    pub fn down(&mut self, a: f32, b: f32) -> f32 {
        let (s0, s1) = self.down.run(b, a);
        0.5 * (s0 + s1)
    }
    #[inline]
    pub fn run(&mut self, x: f32, mut f: impl FnMut(f32) -> f32) -> f32 {
        let (a, b) = self.up(x);
        let (ya, yb) = (f(a), f(b));
        self.down(ya, yb)
    }
    pub fn reset(&mut self) {
        self.up.reset();
        self.down.reset();
    }
}

// -- waveshapers ----------------------------------------------------------------------------------------------------

/// A static nonlinearity with a closed-form antiderivative.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// tanh: the op-amp-and-diodes soft clip.
    Tanh,
    /// tanh on the positive side, `tanh(k x) / k` on the negative: asymmetric (even harmonics), like germanium
    /// diodes or a triode's grid conduction against its cutoff. k < 1 gives the negative side more headroom.
    Asym(f32),
    /// Hard clipping at ±1 (silicon diodes to ground, the RAT and the DS-1).
    Hard,
    /// x / (1 + |x|): a long, soft knee (germanium, a fuzz's rounded top).
    Soft,
}

#[inline]
fn lncosh(x: f64) -> f64 {
    let a = x.abs();
    a + (-2.0 * a).exp().ln_1p() - std::f64::consts::LN_2
}

impl Curve {
    #[inline]
    pub fn f(self, x: f64) -> f64 {
        match self {
            Curve::Tanh => x.tanh(),
            Curve::Asym(k) => {
                if x >= 0.0 {
                    x.tanh()
                } else {
                    let k = k as f64;
                    (k * x).tanh() / k
                }
            }
            Curve::Hard => x.clamp(-1.0, 1.0),
            Curve::Soft => x / (1.0 + x.abs()),
        }
    }
    /// The antiderivative (zero at zero).
    #[inline]
    pub fn integral(self, x: f64) -> f64 {
        match self {
            Curve::Tanh => lncosh(x),
            Curve::Asym(k) => {
                if x >= 0.0 {
                    lncosh(x)
                } else {
                    let k = k as f64;
                    lncosh(k * x) / (k * k)
                }
            }
            Curve::Hard => {
                if x.abs() <= 1.0 {
                    0.5 * x * x
                } else {
                    x.abs() - 0.5
                }
            }
            Curve::Soft => {
                let a = x.abs();
                a - a.ln_1p()
            }
        }
    }
}

/// A waveshaper with first-order antiderivative antialiasing: y = (F(x) - F(x1)) / (x - x1). Pair it with 2x
/// oversampling and the aliases of a hard-driven pedal sit ~30 dB lower than plain oversampling alone.
#[derive(Clone, Copy, Debug)]
pub struct Shaper {
    pub curve: Curve,
    x1: f64,
    f1: f64,
}

impl Shaper {
    pub fn new(curve: Curve) -> Shaper {
        Shaper {
            curve,
            x1: 0.0,
            f1: 0.0,
        }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let fx = self.curve.integral(x);
        let d = x - self.x1;
        let y = if d.abs() > 1e-5 {
            (fx - self.f1) / d
        } else {
            self.curve.f(0.5 * (x + self.x1))
        };
        self.x1 = x;
        self.f1 = fx;
        y as f32
    }
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.f1 = 0.0;
    }
    /// Another curve while running (a tube or diode swap): the remembered antiderivative moves to the new curve, or
    /// the next sample would divide the old curve's difference by a tiny step (a spike).
    pub fn set_curve(&mut self, curve: Curve) {
        if curve != self.curve {
            self.curve = curve;
            self.f1 = curve.integral(self.x1);
        }
    }
    /// Reset to rest at `x` (a stage's bias), so the first sample doesn't step from 0 to it: that step, through the
    /// gain after it, was a thump each time a board started.
    pub fn reset_at(&mut self, x: f32) {
        self.x1 = x as f64;
        self.f1 = self.curve.integral(x as f64);
    }
}

// -- modulation -----------------------------------------------------------------------------------------------------

/// A low-frequency oscillator: a phase 0..1 and its shapes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lfo {
    pub phase: f32,
    inc: f32,
}

impl Lfo {
    pub fn set(&mut self, rate: f32, hz: f32) {
        self.inc = hz / rate;
    }
    #[inline]
    pub fn step(&mut self) -> f32 {
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        self.phase
    }
    /// Advance n samples and return the phase.
    #[inline]
    pub fn step_n(&mut self, n: usize) -> f32 {
        self.phase = (self.phase + self.inc * n as f32).fract();
        self.phase
    }
    #[inline]
    pub fn sine(p: f32) -> f32 {
        (TAU * p).sin()
    }
    #[inline]
    pub fn tri(p: f32) -> f32 {
        1.0 - 4.0 * (p - 0.5).abs()
    }
}

/// A delay line of power-of-two length with linear and cubic (4-point Hermite) fractional reads.
pub struct Line {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
}

impl Line {
    pub fn new(max_samples: usize) -> Line {
        let n = (max_samples + 4).next_power_of_two();
        Line {
            buf: vec![0.0; n],
            mask: n - 1,
            pos: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.pos = (self.pos + 1) & self.mask;
        self.buf[self.pos] = flush(x);
    }
    /// The sample pushed `d` samples ago (d = 0: the newest).
    #[inline]
    pub fn at(&self, d: usize) -> f32 {
        self.buf[(self.pos.wrapping_sub(d)) & self.mask]
    }
    #[inline]
    pub fn linear(&self, d: f32) -> f32 {
        let d = d.clamp(0.0, (self.mask - 3) as f32);
        let i = d as usize;
        let fr = d - i as f32;
        let a = self.at(i);
        a + (self.at(i + 1) - a) * fr
    }
    #[inline]
    pub fn cubic(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, (self.mask - 3) as f32);
        let i = d as usize;
        let t = d - i as f32;
        let (xm1, x0, x1, x2) = (self.at(i - 1), self.at(i), self.at(i + 1), self.at(i + 2));
        let c0 = x0;
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * t + c2) * t + c1) * t + c0
    }
    pub fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// A Schroeder all-pass around a delay line (diffusers and reverb tanks): y = -g x + d, d's input x + g y.
pub struct Diffuser {
    line: Line,
    pub delay: f32,
    pub g: f32,
}

impl Diffuser {
    pub fn new(max: usize, delay: f32, g: f32) -> Diffuser {
        Diffuser {
            line: Line::new(max),
            delay,
            g,
        }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let d = self.line.at(self.delay as usize);
        let v = x + self.g * d;
        self.line.push(v);
        d - self.g * v
    }
    /// With a modulated (fractional) delay.
    #[inline]
    pub fn tick_mod(&mut self, x: f32, delay: f32) -> f32 {
        let d = self.line.linear(delay);
        let v = x + self.g * d;
        self.line.push(v);
        d - self.g * v
    }
    /// A tap inside the line (`d` samples back).
    #[inline]
    pub fn tap(&self, d: usize) -> f32 {
        self.line.at(d)
    }
    pub fn clear(&mut self) {
        self.line.clear();
    }
}

/// The board's last stage: transparent below about -2 dBFS, then a tanh knee to just under full scale.
#[inline]
pub fn soft_limit(x: f32) -> f32 {
    const T: f32 = 0.8;
    const C: f32 = 0.98;
    let a = x.abs();
    if a <= T {
        x
    } else {
        let y = T + (C - T) * ((a - T) / (C - T)).tanh();
        y.copysign(x)
    }
}

/// A DC blocker (one-pole high-pass at a few hertz) for after asymmetric shapers.
#[derive(Clone, Copy, Debug)]
pub struct DcBlock {
    r: f32,
    x1: f32,
    y1: f32,
}

impl DcBlock {
    pub fn new(rate: f32, hz: f32) -> DcBlock {
        DcBlock {
            r: 1.0 - TAU * hz / rate,
            x1: 0.0,
            y1: 0.0,
        }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = flush(y);
        y
    }
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

/// A Linkwitz-Riley 4th-order low-pass or high-pass (two Butterworth sections): an `Lr4` pair splits a signal into
/// bands that sum back flat in level (an all-pass), the crossover the bass blends and the chorus's low cut use.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lr4 {
    s: [Biquad; 2],
}

impl Lr4 {
    pub fn low(rate: f32, f: f32) -> Lr4 {
        let mut l = Lr4::default();
        l.set_low(rate, f);
        l
    }
    pub fn high(rate: f32, f: f32) -> Lr4 {
        let mut l = Lr4::default();
        l.set_high(rate, f);
        l
    }
    pub fn set_low(&mut self, rate: f32, f: f32) {
        for b in &mut self.s {
            b.set(
                rate,
                Filter::LowPass {
                    f,
                    q: std::f32::consts::FRAC_1_SQRT_2,
                },
            );
        }
    }
    pub fn set_high(&mut self, rate: f32, f: f32) {
        for b in &mut self.s {
            b.set(
                rate,
                Filter::HighPass {
                    f,
                    q: std::f32::consts::FRAC_1_SQRT_2,
                },
            );
        }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let t = self.s[0].tick(x);
        self.s[1].tick(t)
    }
    pub fn reset(&mut self) {
        for b in &mut self.s {
            b.reset();
        }
    }
}

/// The "Clean low" choices (`catalog::CLEAN_LOW`) as crossover frequencies; 0 is off (the dry is full band).
pub const CLEAN_LOW_HZ: [f32; 5] = [0.0, 80.0, 120.0, 180.0, 250.0];

/// A drive's clean blend (P37.2, the bass player's Blend and "Clean low"): `blend` of the effect, the rest the dry
/// input; with a crossover the lows under it are only ever the dry signal (the effect's high-passed, the dry split
/// in two), as the Darkglass and Parallax pedals keep a bass's fundamental clean.
#[derive(Clone, Copy, Debug)]
pub struct Blend {
    rate: f32,
    mix: Smooth,
    f: f32,
    dry_lo: Lr4,
    dry_hi: Lr4,
    wet_hi: Lr4,
}

impl Blend {
    pub fn new(rate: f32) -> Blend {
        Blend {
            rate,
            mix: Smooth::new(rate, 25.0, 1.0),
            f: 0.0,
            dry_lo: Lr4::default(),
            dry_hi: Lr4::default(),
            wet_hi: Lr4::default(),
        }
    }
    /// `blend` 0..1 of the effect; `clean_low` an index into `CLEAN_LOW_HZ`.
    pub fn set(&mut self, blend: f32, clean_low: f32) {
        self.mix.target = blend.clamp(0.0, 1.0);
        let f = CLEAN_LOW_HZ[(clean_low.round().max(0.0) as usize).min(CLEAN_LOW_HZ.len() - 1)];
        if f != self.f {
            self.f = f;
            if f > 0.0 {
                self.dry_lo.set_low(self.rate, f);
                self.dry_hi.set_high(self.rate, f);
                self.wet_hi.set_high(self.rate, f);
            }
        }
    }
    /// Whether it changes anything (all effect, no crossover, not gliding: the caller skips it, so the path is
    /// exactly what it was before the knobs came).
    #[inline]
    pub fn active(&self) -> bool {
        self.f > 0.0 || self.mix.target < 1.0 || self.mix.value < 1.0
    }
    #[inline]
    pub fn tick(&mut self, wet: f32, dry: f32) -> f32 {
        let m = self.mix.tick();
        if self.f > 0.0 {
            let lo = self.dry_lo.tick(dry);
            let dh = self.dry_hi.tick(dry);
            let wh = self.wet_hi.tick(wet);
            lo + wh * m + dh * (1.0 - m)
        } else {
            wet * m + dry * (1.0 - m)
        }
    }
    pub fn reset(&mut self) {
        self.mix.snap();
        self.dry_lo.reset();
        self.dry_hi.reset();
        self.wet_hi.reset();
    }
}

/// A knob position 0..10 as 0..1.
#[inline]
pub fn unit10(v: f32) -> f32 {
    (v / 10.0).clamp(0.0, 1.0)
}

/// An audio-taper pot (log): 0..1 to 0..1, about 10% at half way.
#[inline]
pub fn audio_taper(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    (u * u * 0.6 + u * u * u * 0.4).min(1.0)
}

/// Exponential sweep between two frequencies (u in 0..1).
#[inline]
pub fn sweep(lo: f32, hi: f32, u: f32) -> f32 {
    lo * (hi / lo).powf(u.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tone::dsp::{rms, sine};

    /// Level of `hz` in `x` (a single-bin DFT, Hann window).
    pub(crate) fn tone_level(x: &[f32], rate: f32, hz: f32) -> f32 {
        let n = x.len();
        let (mut re, mut im, mut wsum) = (0.0f64, 0.0f64, 0.0f64);
        for (i, &v) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (TAU as f64 * i as f64 / n as f64).cos();
            let ph = TAU as f64 * hz as f64 * i as f64 / rate as f64;
            re += v as f64 * w * ph.cos();
            im += v as f64 * w * ph.sin();
            wsum += w;
        }
        (2.0 * (re * re + im * im).sqrt() / wsum) as f32
    }

    #[test]
    fn halfband_rejects_images_and_passes_the_band() {
        let r = 48000.0;
        // upsample a 15 kHz tone: at 96 kHz the image at 81 kHz must be far down, the tone kept
        let x = sine(r, 15000.0, 24000, 0.5);
        let mut os = Oversample2::new();
        let mut up = Vec::with_capacity(48000);
        for &s in &x {
            let (a, b) = os.up(s);
            up.push(a);
            up.push(b);
        }
        let tail = &up[8000..];
        let keep = tone_level(tail, 2.0 * r, 15000.0);
        let image = tone_level(tail, 2.0 * r, 33000.0);
        assert!((keep - 0.5).abs() < 0.01, "passband {keep}");
        assert!(image < 0.5 * 10f32.powf(-80.0 / 20.0), "image {image}");
        // and down again: a 30 kHz tone at 96 kHz (which would alias to 18 kHz) is removed
        // (the phase in f64: an f32 sin of a phase this large carries its own broadband noise)
        let hi: Vec<f32> = (0..48000)
            .map(|i| ((std::f64::consts::TAU * 30000.0 * i as f64 / 96000.0).sin() * 0.5) as f32)
            .collect();
        let mut os = Oversample2::new();
        let down: Vec<f32> = hi.chunks(2).map(|c| os.down(c[0], c[1])).collect();
        assert!(
            rms(&down[4000..]) < 0.5 * 10f32.powf(-80.0 / 20.0),
            "alias {}",
            rms(&down[4000..])
        );
    }

    #[test]
    fn oversampling_round_trip_keeps_a_tone() {
        let r = 48000.0;
        let x = sine(r, 3000.0, 24000, 0.3);
        let mut os = Oversample2::new();
        let y: Vec<f32> = x.iter().map(|&s| os.run(s, |v| v)).collect();
        assert!((rms(&y[4000..]) - rms(&x[4000..])).abs() < 0.003);
    }

    #[test]
    fn adaa_matches_the_curve_on_slow_signals_and_cuts_aliasing() {
        for curve in [Curve::Tanh, Curve::Asym(0.6), Curve::Hard, Curve::Soft] {
            let mut sh = Shaper::new(curve);
            // a slow ramp: ADAA ≈ f at the midpoint
            let mut last = 0.0;
            for i in 0..2000 {
                let x = -3.0 + 6.0 * i as f32 / 2000.0;
                let y = sh.tick(x);
                if i > 1 {
                    let want = curve.f((x - 0.0015) as f64) as f32;
                    assert!((y - want).abs() < 2e-3, "{curve:?} at {x}: {y} vs {want}");
                }
                last = y;
            }
            assert!(last.is_finite());
        }
        // a 5 kHz sine driven hard: aliases (energy away from the harmonics) much lower with ADAA + 2x
        let r = 48000.0;
        let x = sine(r, 4999.0, 48000, 1.0);
        let plain: Vec<f32> = x.iter().map(|&v| (v * 8.0).clamp(-1.0, 1.0)).collect();
        let mut os = Oversample2::new();
        let mut sh = Shaper::new(Curve::Hard);
        let good: Vec<f32> = x.iter().map(|&v| os.run(v, |s| sh.tick(s * 8.0))).collect();
        // 4999 Hz × 5 = 24995 → aliases to 23005 Hz; × 7 = 34993 → 13007 Hz
        let a_plain = tone_level(&plain[2000..], r, 13007.0);
        let a_good = tone_level(&good[2000..], r, 13007.0);
        assert!(a_good < a_plain * 0.1, "alias {a_good} vs {a_plain}");
    }

    #[test]
    fn svf_and_one_pole_are_cutoff_exact() {
        let r = 48000.0;
        let mut lp = OnePole::new(r, 1000.0);
        let y: Vec<f32> = sine(r, 1000.0, 48000, 1.0).iter().map(|&x| lp.lp(x)).collect();
        let db = 20.0 * (rms(&y[24000..]) / (1.0 / 2f32.sqrt())).log10();
        assert!((db + 3.01).abs() < 0.1, "{db}");
        let mut s = Svf::new(r, 800.0, 5.0);
        let y: Vec<f32> = sine(r, 800.0, 48000, 1.0).iter().map(|&x| s.bp_norm(x)).collect();
        assert!((rms(&y[24000..]) - 1.0 / 2f32.sqrt()).abs() < 0.01);
    }

    #[test]
    fn cubic_reads_interpolate() {
        let mut l = Line::new(64);
        for i in 0..64 {
            l.push(i as f32);
        }
        assert_eq!(l.at(0), 63.0);
        assert!((l.cubic(2.5) - 60.5).abs() < 1e-4);
        assert!((l.linear(10.25) - 52.75).abs() < 1e-4);
    }
}
