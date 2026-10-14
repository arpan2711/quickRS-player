//! DSP building blocks for the tone (all allocation happens in `new`; `process` never allocates).

use std::f32::consts::{PI, TAU};

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[inline]
fn flush(x: f32) -> f32 {
    // keep denormals out of feedback paths
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// RBJ cookbook biquad (transposed direct form II).
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum Filter {
    LowPass { f: f32, q: f32 },
    HighPass { f: f32, q: f32 },
    Peak { f: f32, q: f32, db: f32 },
    LowShelf { f: f32, db: f32 },
    HighShelf { f: f32, db: f32 },
}

impl Biquad {
    pub fn new(rate: f32, kind: Filter) -> Biquad {
        let mut b = Biquad::default();
        b.set(rate, kind);
        b
    }

    /// New coefficients, keeping the filter's state (no click).
    pub fn set(&mut self, rate: f32, kind: Filter) {
        let f = match kind {
            Filter::LowPass { f, .. } | Filter::HighPass { f, .. } | Filter::Peak { f, .. } => f,
            Filter::LowShelf { f, .. } | Filter::HighShelf { f, .. } => f,
        };
        let w0 = TAU * (f / rate).clamp(1e-5, 0.49);
        let (cw, sw) = (w0.cos(), w0.sin());
        let (b0, b1, b2, a0, a1, a2) = match kind {
            Filter::LowPass { q, .. } => {
                let al = sw / (2.0 * q);
                (
                    (1.0 - cw) / 2.0,
                    1.0 - cw,
                    (1.0 - cw) / 2.0,
                    1.0 + al,
                    -2.0 * cw,
                    1.0 - al,
                )
            }
            Filter::HighPass { q, .. } => {
                let al = sw / (2.0 * q);
                (
                    (1.0 + cw) / 2.0,
                    -(1.0 + cw),
                    (1.0 + cw) / 2.0,
                    1.0 + al,
                    -2.0 * cw,
                    1.0 - al,
                )
            }
            Filter::Peak { q, db, .. } => {
                let a = 10f32.powf(db / 40.0);
                let al = sw / (2.0 * q);
                (
                    1.0 + al * a,
                    -2.0 * cw,
                    1.0 - al * a,
                    1.0 + al / a,
                    -2.0 * cw,
                    1.0 - al / a,
                )
            }
            Filter::LowShelf { db, .. } => {
                let a = 10f32.powf(db / 40.0);
                let al = sw / 2.0 * (2f32).sqrt();
                let s = 2.0 * a.sqrt() * al;
                (
                    a * ((a + 1.0) - (a - 1.0) * cw + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
                    a * ((a + 1.0) - (a - 1.0) * cw - s),
                    (a + 1.0) + (a - 1.0) * cw + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cw),
                    (a + 1.0) + (a - 1.0) * cw - s,
                )
            }
            Filter::HighShelf { db, .. } => {
                let a = 10f32.powf(db / 40.0);
                let al = sw / 2.0 * (2f32).sqrt();
                let s = 2.0 * a.sqrt() * al;
                (
                    a * ((a + 1.0) + (a - 1.0) * cw + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
                    a * ((a + 1.0) + (a - 1.0) * cw - s),
                    (a + 1.0) - (a - 1.0) * cw + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cw),
                    (a + 1.0) - (a - 1.0) * cw - s,
                )
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = flush(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush(self.b2 * x - self.a2 * y);
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Envelope follower: separate attack and release time constants.
#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    att: f32,
    rel: f32,
    pub value: f32,
}

impl Envelope {
    pub fn new(rate: f32, attack_ms: f32, release_ms: f32) -> Envelope {
        let c = |ms: f32| (-1.0 / (rate * ms.max(0.01) * 1e-3)).exp();
        Envelope {
            att: c(attack_ms),
            rel: c(release_ms),
            value: 0.0,
        }
    }
    #[inline]
    pub fn tick(&mut self, level: f32) -> f32 {
        let c = if level > self.value { self.att } else { self.rel };
        self.value = flush(level + (self.value - level) * c);
        self.value
    }
}

/// Downward expander / noise gate with hysteresis and a smooth gain.
pub struct Gate {
    env: Envelope,
    gain: Envelope,
    pub threshold: f32,
    open: bool,
    ratio: f32,
}

impl Gate {
    pub fn new(rate: f32) -> Gate {
        Gate {
            env: Envelope::new(rate, 0.5, 60.0),
            gain: Envelope::new(rate, 1.0, 110.0),
            threshold: db_to_gain(-56.0),
            open: false,
            ratio: 6.0,
        }
    }
    pub fn set_threshold_db(&mut self, db: f32) {
        self.threshold = if db <= -79.0 { 0.0 } else { db_to_gain(db) };
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        if self.threshold <= 0.0 {
            return x;
        }
        let e = self.env.tick(x.abs());
        if e > self.threshold {
            self.open = true;
        } else if e < self.threshold * 0.5 {
            self.open = false;
        }
        let target = if self.open {
            1.0
        } else {
            // below the threshold: expand downward (ratio:1)
            let below = (e / self.threshold).max(1e-6);
            below.powf(self.ratio - 1.0).min(1.0)
        };
        x * self.gain.tick(target)
    }
}

/// Feed-forward compressor on the signal's envelope, with makeup gain.
pub struct Compressor {
    env: Envelope,
    pub threshold_db: f32,
    pub ratio: f32,
    pub makeup: f32,
}

impl Compressor {
    pub fn new(rate: f32) -> Compressor {
        Compressor {
            env: Envelope::new(rate, 4.0, 140.0),
            threshold_db: -20.0,
            ratio: 3.0,
            makeup: 1.0,
        }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let e = self.env.tick(x.abs()).max(1e-6);
        let db = 20.0 * e.log10();
        let over = db - self.threshold_db;
        let gain_db = if over > 0.0 {
            -over * (1.0 - 1.0 / self.ratio)
        } else {
            0.0
        };
        x * db_to_gain(gain_db) * self.makeup
    }
}

/// Saturation with 4x oversampling (polyphase-free: zero-stuff, lowpass, shape, lowpass, decimate).
pub struct Drive {
    pub gain: f32,
    /// Asymmetry (adds even harmonics like a tube stage).
    pub bias: f32,
    up: [Biquad; 4],
    down: [Biquad; 4],
    dc: Biquad,
}

/// Q of each section of an 8th-order Butterworth low-pass.
const BUTTER8: [f32; 4] = [0.5098, 0.6013, 0.8999, 2.5629];

impl Drive {
    pub fn new(rate: f32) -> Drive {
        let r4 = rate * 4.0;
        // 8th-order Butterworth at 0.42 x the base rate, before and after the shaper
        let lp = |q: f32| Biquad::new(r4, Filter::LowPass { f: rate * 0.42, q });
        let bank = || [lp(BUTTER8[0]), lp(BUTTER8[1]), lp(BUTTER8[2]), lp(BUTTER8[3])];
        Drive {
            gain: 1.0,
            bias: 0.12,
            up: bank(),
            down: bank(),
            dc: Biquad::new(rate, Filter::HighPass { f: 20.0, q: 0.707 }),
        }
    }
    #[inline]
    fn shape(&self, x: f32) -> f32 {
        let b = self.bias;
        // tanh of the driven signal, re-centred so silence stays silent
        (x * self.gain + b).tanh() - b.tanh()
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let mut out = 0.0;
        for k in 0..4 {
            let mut s = if k == 0 { x * 4.0 } else { 0.0 };
            for f in &mut self.up {
                s = f.tick(s);
            }
            let mut y = self.shape(s);
            for f in &mut self.down {
                y = f.tick(y);
            }
            if k == 3 {
                out = y;
            }
        }
        self.dc.tick(out)
    }
}

/// A delay line with fractional reads (chorus, delay, comb filters).
pub struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    pub fn new(max_samples: usize) -> DelayLine {
        DelayLine {
            buf: vec![0.0; max_samples.max(4) + 2],
            pos: 0,
        }
    }
    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.pos] = flush(x);
        self.pos = (self.pos + 1) % self.buf.len();
    }
    /// The sample written `d` samples ago (fractional, linear interpolation).
    #[inline]
    pub fn read(&self, d: f32) -> f32 {
        let n = self.buf.len();
        let d = d.clamp(1.0, (n - 2) as f32);
        let i = d.floor() as usize;
        let frac = d - i as f32;
        let a = self.buf[(self.pos + n - i) % n];
        let b = self.buf[(self.pos + n - i - 1) % n];
        a + (b - a) * frac
    }
}

pub struct Chorus {
    line: DelayLine,
    phase: f32,
    rate: f32,
    pub mix: f32,
    pub speed_hz: f32,
    pub depth_ms: f32,
}

impl Chorus {
    pub fn new(rate: f32) -> Chorus {
        Chorus {
            line: DelayLine::new((rate * 0.05) as usize),
            phase: 0.0,
            rate,
            mix: 0.0,
            speed_hz: 0.9,
            depth_ms: 3.0,
        }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        self.line.write(x);
        self.phase = (self.phase + self.speed_hz / self.rate) % 1.0;
        let d = (15.0 + self.depth_ms * (TAU * self.phase).sin()) * 1e-3 * self.rate;
        x * (1.0 - self.mix * 0.5) + self.line.read(d) * self.mix
    }
}

pub struct Echo {
    line: DelayLine,
    rate: f32,
    damp: Biquad,
    pub time_s: f32,
    pub feedback: f32,
    pub mix: f32,
    smooth_d: f32,
}

impl Echo {
    pub fn new(rate: f32) -> Echo {
        Echo {
            line: DelayLine::new((rate * 1.0) as usize),
            rate,
            damp: Biquad::new(rate, Filter::LowPass { f: 4500.0, q: 0.707 }),
            time_s: 0.32,
            feedback: 0.3,
            mix: 0.0,
            smooth_d: 0.32 * rate,
        }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        // glide to a new delay time instead of jumping (tape-style pitch bend rather than a click)
        self.smooth_d += (self.time_s * self.rate - self.smooth_d) * 0.0005;
        let wet = self.line.read(self.smooth_d);
        self.line.write(x + self.damp.tick(wet) * self.feedback.min(0.95));
        x + wet * self.mix
    }
}

/// Freeverb (Jezar's), mono: 8 damped combs in parallel, then 4 allpasses.
pub struct Reverb {
    combs: Vec<(DelayLine, usize, f32)>,
    alls: Vec<(DelayLine, usize)>,
    pub room: f32,
    pub damp: f32,
    pub wet: f32,
    pub dry: f32,
}

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];

impl Reverb {
    pub fn new(rate: f32) -> Reverb {
        let s = rate / 44100.0;
        Reverb {
            combs: COMBS
                .iter()
                .map(|&n| {
                    let n = (n as f32 * s) as usize;
                    (DelayLine::new(n + 2), n, 0.0)
                })
                .collect(),
            alls: ALLPASSES
                .iter()
                .map(|&n| {
                    let n = (n as f32 * s) as usize;
                    (DelayLine::new(n + 2), n)
                })
                .collect(),
            room: 0.3,
            damp: 0.4,
            wet: 0.0,
            dry: 1.0,
        }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        if self.wet <= 0.0 {
            return x;
        }
        let feedback = 0.7 + 0.28 * self.room.clamp(0.0, 1.0);
        let input = x * 0.015;
        let mut out = 0.0;
        for (line, n, store) in &mut self.combs {
            let y = line.read(*n as f32);
            *store = flush(y * (1.0 - self.damp) + *store * self.damp);
            line.write(input + *store * feedback);
            out += y;
        }
        for (line, n) in &mut self.alls {
            let b = line.read(*n as f32);
            line.write(out + b * 0.5);
            out = b - out;
        }
        x * self.dry + out * self.wet * 3.0
    }
}

/// A sine for tests.
pub fn sine(rate: f32, hz: f32, n: usize, amp: f32) -> Vec<f32> {
    (0..n).map(|i| (TAU * hz * i as f32 / rate).sin() * amp).collect()
}

/// Root mean square.
pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[allow(dead_code)]
const _: f32 = PI;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biquad_lowpass_passes_low_cuts_high() {
        let r = 48000.0;
        let mut lp = Biquad::new(r, Filter::LowPass { f: 1000.0, q: 0.707 });
        let low: Vec<f32> = sine(r, 100.0, 9600, 0.5).iter().map(|&x| lp.tick(x)).collect();
        lp.reset();
        let high: Vec<f32> = sine(r, 10000.0, 9600, 0.5).iter().map(|&x| lp.tick(x)).collect();
        assert!((rms(&low[4800..]) - 0.3535).abs() < 0.02);
        assert!(rms(&high[4800..]) < 0.01);
    }

    #[test]
    fn shelves_and_peaks_change_level_as_asked() {
        let r = 48000.0;
        let mut ls = Biquad::new(r, Filter::LowShelf { f: 200.0, db: 6.0 });
        let y: Vec<f32> = sine(r, 40.0, 48000, 0.25).iter().map(|&x| ls.tick(x)).collect();
        let gain_db = 20.0 * (rms(&y[24000..]) / (0.25 / 2f32.sqrt())).log10();
        assert!((gain_db - 6.0).abs() < 0.6, "{gain_db}");
        let mut pk = Biquad::new(
            r,
            Filter::Peak {
                f: 1000.0,
                q: 1.0,
                db: -6.0,
            },
        );
        let y: Vec<f32> = sine(r, 1000.0, 48000, 0.25).iter().map(|&x| pk.tick(x)).collect();
        let gain_db = 20.0 * (rms(&y[24000..]) / (0.25 / 2f32.sqrt())).log10();
        assert!((gain_db + 6.0).abs() < 0.3, "{gain_db}");
    }

    #[test]
    fn gate_closes_on_noise_and_opens_on_signal() {
        let r = 48000.0;
        let mut g = Gate::new(r);
        g.set_threshold_db(-40.0);
        let noise: Vec<f32> = (0..24000)
            .map(|i| if i % 2 == 0 { 0.001 } else { -0.001 })
            .collect();
        let y: Vec<f32> = noise.iter().map(|&x| g.tick(x)).collect();
        assert!(rms(&y[20000..]) < 0.0002);
        let s = sine(r, 220.0, 24000, 0.3);
        let y: Vec<f32> = s.iter().map(|&x| g.tick(x)).collect();
        assert!(rms(&y[12000..]) > 0.2);
    }

    #[test]
    fn drive_is_bounded_and_silent_on_silence() {
        let r = 48000.0;
        let mut d = Drive::new(r);
        d.gain = db_to_gain(40.0);
        let y: Vec<f32> = sine(r, 110.0, 48000, 0.5).iter().map(|&x| d.tick(x)).collect();
        assert!(y.iter().all(|v| v.is_finite() && v.abs() < 2.5));
        let z: Vec<f32> = vec![0.0f32; 4800].iter().map(|&x| d.tick(x)).collect();
        assert!(rms(&z[2400..]) < 1e-3);
    }

    #[test]
    fn reverb_and_echo_ring_out_and_stay_finite() {
        let r = 48000.0;
        let mut rv = Reverb::new(r);
        rv.wet = 0.3;
        rv.room = 0.9;
        let mut e = Echo::new(r);
        e.mix = 0.4;
        e.feedback = 0.9;
        let mut x = vec![0.0f32; 96000];
        x[0] = 1.0;
        let y: Vec<f32> = x.iter().map(|&s| e.tick(rv.tick(s))).collect();
        assert!(y.iter().all(|v| v.is_finite()));
        assert!(rms(&y[1000..20000]) > 1e-4, "tail rings");
        assert!(y.iter().all(|v| v.abs() < 4.0));
    }
}
