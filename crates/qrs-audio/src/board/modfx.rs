//! Modulation: chorus, flanger, phaser, tremolo, Uni-Vibe, rotary speaker, octaver.
//!
//! - Chorus and flanger: a delay line swept by an LFO and read with cubic interpolation; the chorus's wet path is
//!   darkened like a bucket-brigade chip's, the flanger feeds back.
//! - Phaser: four first-order all-passes sharing one swept cutoff (the Phase 90's matched JFETs), feedback, and
//!   the classic half-dry mix that makes two notches.
//! - Vibe: four all-passes at staggered frequencies (the Uni-Vibe's unequal capacitors) swept by a lamp: an LFO
//!   through a filament's thermal lag, so the sweep is lopsided and "throbs".
//! - Rotary: a 800 Hz crossover; the horn and the drum each spin with their own inertia (chorale and tremolo
//!   speeds as Leslie's), each giving Doppler (a modulated delay) and amplitude modulation.
//! - Octaver: the OC-2's trick: a flip-flop on the zero crossings of the low-passed input halves its frequency and
//!   flips the sign of the input with it (the octave down); full-wave rectifying gives the octave up. A second
//!   flip-flop on the first gives the OC-2's "Oct 2" (two octaves down). P37.2: the tracker's low-pass follows the
//!   note it finds (a 4-pole at 0.8 of it), so a bass note whose 2nd harmonic is louder than its fundamental is
//!   still tracked at the fundamental; its high-pass is 22 Hz (was 50, which took 8 dB off a low B), and the sub
//!   fades out under a tracking floor instead of sputtering on the decay.
//! - Chorus: a Low cut (P37.2, the Boss CEB-3's) keeps everything under it dry, for bass.

use std::f32::consts::{PI, TAU};

use super::catalog::LOW_CUT_HZ;
use super::dsp::{Biquad, Filter, Lfo, Line, Lr4, OnePole, Smooth, coef_ms, flush, sweep, unit10};
use super::{MAX_KNOBS, Pedal};

// -- Chorus ---------------------------------------------------------------------------------------------------------

struct Chorus {
    rate: f32,
    line: Line,
    lfo: Lfo,
    depth: Smooth,
    mix: Smooth,
    wet_lp: Biquad,
    centre: f32,
    /// The low cut: the band under it passes dry (0: off, the whole signal is chorused as before).
    cut: f32,
    lo: Lr4,
    hi: Lr4,
}

pub fn chorus(rate: f32) -> Box<dyn Pedal> {
    Box::new(Chorus {
        rate,
        line: Line::new((rate * 0.03) as usize),
        lfo: Lfo::default(),
        depth: Smooth::new(rate, 50.0, 0.0),
        mix: Smooth::new(rate, 30.0, 0.5),
        wet_lp: Biquad::new(rate, Filter::LowPass { f: 7000.0, q: 0.7 }),
        centre: 0.0075 * rate,
        cut: 0.0,
        lo: Lr4::default(),
        hi: Lr4::default(),
    })
}

impl Pedal for Chorus {
    fn process(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            let low = if self.cut > 0.0 {
                let l = self.lo.tick(*v);
                *v = self.hi.tick(*v);
                l
            } else {
                0.0
            };
            self.line.push(*v);
            let p = self.lfo.step();
            // a rounded triangle, like the CE-2's
            let w = Lfo::tri(p);
            let w = w * (1.5 - 0.5 * w * w);
            let d = self.centre + self.depth.tick() * w;
            let wet = self.wet_lp.tick(self.line.cubic(d));
            let m = self.mix.tick();
            *v = *v * (1.0 - 0.5 * m) + wet * m;
            if self.cut > 0.0 {
                *v += low;
            }
        }
    }
    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        self.lfo.set(self.rate, sweep(0.12, 6.0, unit10(v[0])));
        self.depth.target = unit10(v[1]) * 0.0038 * self.rate;
        self.mix.target = (v[2] / 100.0).clamp(0.0, 1.0);
        let cut = LOW_CUT_HZ[(v[3].round().max(0.0) as usize).min(LOW_CUT_HZ.len() - 1)];
        if cut != self.cut {
            self.cut = cut;
            if cut > 0.0 {
                self.lo.set_low(self.rate, cut);
                self.hi.set_high(self.rate, cut);
            }
        }
    }
    fn reset(&mut self) {
        self.line.clear();
        self.wet_lp.reset();
        self.lo.reset();
        self.hi.reset();
        self.depth.snap();
        self.mix.snap();
    }
}

// -- Flanger --------------------------------------------------------------------------------------------------------

struct Flanger {
    rate: f32,
    line: Line,
    lfo: Lfo,
    depth: Smooth,
    /// Manual: the sweep's shortest delay, samples.
    base: Smooth,
    fb: f32,
    mix: Smooth,
    last: f32,
    damp: OnePole,
}

pub fn flanger(rate: f32) -> Box<dyn Pedal> {
    Box::new(Flanger {
        rate,
        line: Line::new((rate * 0.012) as usize),
        lfo: Lfo::default(),
        depth: Smooth::new(rate, 50.0, 0.0),
        base: Smooth::new(rate, 50.0, 0.00025 * rate),
        fb: 0.5,
        mix: Smooth::new(rate, 30.0, 0.5),
        last: 0.0,
        damp: OnePole::new(rate, 9000.0),
    })
}

impl Pedal for Flanger {
    fn process(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            self.line.push(*v + self.fb * self.last);
            let p = self.lfo.step();
            let w = 0.5 + 0.5 * Lfo::tri(p);
            let d = self.base.tick() + self.depth.tick() * w * w;
            let wet = self.damp.lp(self.line.cubic(d));
            self.last = flush(wet);
            let m = self.mix.tick();
            *v = (*v + wet * m) / (1.0 + 0.6 * m);
        }
    }
    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        self.lfo.set(self.rate, sweep(0.05, 4.0, unit10(v[0])));
        self.depth.target = unit10(v[1]) * 0.0045 * self.rate;
        self.fb = 0.9 * unit10(v[2]);
        self.mix.target = (v[3] / 100.0).clamp(0.0, 1.0);
        // (0.25 ms, the Manual it always had, unless set)
        let manual = if v[4] > 0.0 { v[4].clamp(0.1, 4.0) } else { 0.25 };
        self.base.target = if (manual - 0.25).abs() < 1e-6 {
            0.00025 * self.rate
        } else {
            manual * 1e-3 * self.rate
        };
    }
    fn reset(&mut self) {
        self.line.clear();
        self.last = 0.0;
        self.damp.reset();
        self.depth.snap();
        self.base.snap();
        self.mix.snap();
    }
}

// -- Phaser ---------------------------------------------------------------------------------------------------------

struct Phaser {
    rate: f32,
    ap: [OnePole; 4],
    lfo: Lfo,
    lo: f32,
    hi: f32,
    fb: f32,
    last: f32,
    mix: Smooth,
    count: usize,
}

pub fn phaser(rate: f32) -> Box<dyn Pedal> {
    Box::new(Phaser {
        rate,
        ap: [OnePole::default(); 4],
        lfo: Lfo::default(),
        lo: 160.0,
        hi: 2200.0,
        fb: 0.25,
        last: 0.0,
        mix: Smooth::new(rate, 30.0, 0.5),
        count: 0,
    })
}

impl Pedal for Phaser {
    fn process(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            if self.count == 0 {
                let p = self.lfo.step_n(8);
                let w = 0.5 + 0.5 * Lfo::sine(p);
                let f = sweep(self.lo, self.hi, w);
                for a in &mut self.ap {
                    a.set(self.rate, f);
                }
                self.count = 8;
            }
            self.count -= 1;
            let mut s = *v + self.fb * self.last;
            for a in &mut self.ap {
                s = a.ap(s);
            }
            self.last = flush(s);
            let m = self.mix.tick();
            *v = *v * (1.0 - 0.5 * m) + s * 0.5 * m;
        }
    }
    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        self.lfo.set(self.rate, sweep(0.08, 8.0, unit10(v[0])));
        let d = unit10(v[1]);
        self.lo = 160.0;
        self.hi = 160.0 * (1.0 + 16.0 * d);
        self.mix.target = (v[2] / 100.0).clamp(0.0, 1.0);
    }
    fn reset(&mut self) {
        for a in &mut self.ap {
            a.reset();
        }
        self.last = 0.0;
        self.mix.snap();
    }
}

// -- Tremolo --------------------------------------------------------------------------------------------------------

struct Tremolo {
    rate: f32,
    lfo: Lfo,
    depth: Smooth,
    wave: i32,
    smooth: OnePole,
}

pub fn tremolo(rate: f32) -> Box<dyn Pedal> {
    Box::new(Tremolo {
        rate,
        lfo: Lfo::default(),
        depth: Smooth::new(rate, 30.0, 0.5),
        wave: 0,
        smooth: OnePole::new(rate, 120.0),
    })
}

impl Pedal for Tremolo {
    fn process(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            let p = self.lfo.step();
            let w = match self.wave {
                1 => Lfo::tri(p),
                2 => (4.0 * Lfo::sine(p)).tanh(),
                _ => Lfo::sine(p),
            };
            // the square softened so it chops without clicking
            let w = if self.wave == 2 { self.smooth.lp(w) } else { w };
            let d = self.depth.tick();
            *v *= 1.0 - d * (0.5 - 0.5 * w);
        }
    }
    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        self.lfo.set(self.rate, sweep(1.0, 12.0, unit10(v[0])));
        self.depth.target = unit10(v[1]);
        self.wave = v[2].round() as i32;
    }
    fn reset(&mut self) {
        self.smooth.reset();
        self.depth.snap();
    }
}

// -- Vibe -----------------------------------------------------------------------------------------------------------

/// The four stages' base frequencies, staggered like the Uni-Vibe's capacitors.
const VIBE_BASE: [f32; 4] = [95.0, 380.0, 1400.0, 3600.0];

struct Vibe {
    rate: f32,
    ap: [OnePole; 4],
    lfo: Lfo,
    lamp: f32,
    lamp_up: f32,
    lamp_down: f32,
    intensity: f32,
    vibrato: bool,
    count: usize,
}

pub fn vibe(rate: f32) -> Box<dyn Pedal> {
    Box::new(Vibe {
        rate,
        ap: [OnePole::default(); 4],
        lfo: Lfo::default(),
        lamp: 0.0,
        lamp_up: coef_ms(rate / 8.0, 6.0),
        lamp_down: coef_ms(rate / 8.0, 35.0),
        intensity: 0.7,
        vibrato: false,
        count: 0,
    })
}

impl Pedal for Vibe {
    fn process(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            if self.count == 0 {
                let p = self.lfo.step_n(8);
                // the lamp brightens fast and dims slowly (its filament), the cells follow the light
                let drive = (0.5 + 0.5 * Lfo::sine(p)).powf(1.6);
                let c = if drive > self.lamp {
                    self.lamp_up
                } else {
                    self.lamp_down
                };
                self.lamp += (drive - self.lamp) * c;
                let m = 0.35 + 2.4 * self.intensity * self.lamp;
                for (a, f) in self.ap.iter_mut().zip(VIBE_BASE) {
                    a.set(self.rate, f * m);
                }
                self.count = 8;
            }
            self.count -= 1;
            let mut s = *v;
            for a in &mut self.ap {
                s = a.ap(s);
            }
            // a little of the lamp's throb in the level
            let throb = 1.0 - 0.08 * self.intensity * self.lamp;
            *v = if self.vibrato { s } else { 0.5 * (*v + s) } * throb;
        }
    }
    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        self.lfo.set(self.rate, sweep(0.6, 9.0, unit10(v[0])));
        self.intensity = unit10(v[1]);
        self.vibrato = v[2] > 0.5;
    }
    fn reset(&mut self) {
        for a in &mut self.ap {
            a.reset();
        }
        self.lamp = 0.0;
    }
}

// -- Rotary ---------------------------------------------------------------------------------------------------------

struct Rotary {
    rate: f32,
    lo: [Biquad; 2],
    hi: [Biquad; 2],
    horn_line: Line,
    drum_line: Line,
    horn_phase: f32,
    drum_phase: f32,
    horn_hz: f32,
    drum_hz: f32,
    horn_target: f32,
    drum_target: f32,
    horn_acc: f32,
    drum_acc: f32,
    horn_lp: OnePole,
    balance: f32,
    depth: f32,
    mix: Smooth,
    count: usize,
}

pub fn rotary(rate: f32) -> Box<dyn Pedal> {
    let q = std::f32::consts::FRAC_1_SQRT_2;
    Box::new(Rotary {
        rate,
        lo: [Biquad::new(rate, Filter::LowPass { f: 800.0, q }); 2],
        hi: [Biquad::new(rate, Filter::HighPass { f: 800.0, q }); 2],
        horn_line: Line::new((rate * 0.006) as usize),
        drum_line: Line::new((rate * 0.006) as usize),
        horn_phase: 0.0,
        drum_phase: 0.25,
        horn_hz: 0.8,
        drum_hz: 0.67,
        horn_target: 0.8,
        drum_target: 0.67,
        // the horn spins up in about a second, the heavy drum in several
        horn_acc: coef_ms(rate / 16.0, 700.0),
        drum_acc: coef_ms(rate / 16.0, 3500.0),
        horn_lp: OnePole::new(rate, 8000.0),
        balance: 0.5,
        depth: 0.6,
        mix: Smooth::new(rate, 30.0, 0.7),
        count: 0,
    })
}

impl Pedal for Rotary {
    fn process(&mut self, x: &mut [f32]) {
        let r = self.rate;
        for v in x.iter_mut() {
            if self.count == 0 {
                self.horn_hz += (self.horn_target - self.horn_hz) * self.horn_acc;
                self.drum_hz += (self.drum_target - self.drum_hz) * self.drum_acc;
                self.count = 16;
            }
            self.count -= 1;
            self.horn_phase = (self.horn_phase + self.horn_hz / r).fract();
            self.drum_phase = (self.drum_phase + self.drum_hz / r).fract();
            let l = {
                let t0 = self.lo[0].tick(*v);
                self.lo[1].tick(t0)
            };
            let h = {
                let t0 = self.hi[0].tick(*v);
                self.hi[1].tick(t0)
            };
            let (hs, hc) = (TAU * self.horn_phase).sin_cos();
            let (ds, dc) = (TAU * self.drum_phase).sin_cos();
            let dep = self.depth;
            // Doppler: the horn's mouth moves about 15 cm towards and away from you
            self.horn_line.push(h);
            let hd = (0.0012 + 0.00035 * dep * hs) * r;
            let horn = self.horn_lp.lp(self.horn_line.cubic(hd)) * (1.0 - 0.35 * dep * (0.5 + 0.5 * hc));
            self.drum_line.push(l);
            let dd = (0.0012 + 0.00012 * dep * ds) * r;
            let drum = self.drum_line.cubic(dd) * (1.0 - 0.22 * dep * (0.5 + 0.5 * dc));
            let wet = horn * (0.5 + self.balance) + drum * (1.5 - self.balance);
            let m = self.mix.tick();
            *v = *v * (1.0 - m) + wet * m;
        }
        let _ = PI;
    }
    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        let fast = v[0] > 0.5;
        self.horn_target = if fast { 6.8 } else { 0.8 };
        self.drum_target = if fast { 5.9 } else { 0.67 };
        self.balance = unit10(v[1]);
        self.depth = unit10(v[2]);
        self.mix.target = (v[3] / 100.0).clamp(0.0, 1.0);
    }
    fn reset(&mut self) {
        for b in self.lo.iter_mut().chain(self.hi.iter_mut()) {
            b.reset();
        }
        self.horn_line.clear();
        self.drum_line.clear();
        self.horn_hz = self.horn_target;
        self.drum_hz = self.drum_target;
        self.mix.snap();
    }
}

// -- Octaver --------------------------------------------------------------------------------------------------------

/// The tracker's low-pass with no note found yet, and its range.
const TRACK_OPEN: f32 = 320.0;
const TRACK_MIN: f32 = 25.0;
/// Under this the tracked signal is noise or a dying note: the sub fades out.
const TRACK_FLOOR: f32 = 0.0015;

struct Octaver {
    rate: f32,
    hp: Biquad,
    track: [Biquad; 2],
    /// The tracker's low-pass now (follows the note found: 0.8 of it).
    fc: f32,
    /// Samples since the last rising crossing.
    since: u32,
    sub_lp: [Biquad; 2],
    sub2_lp: [Biquad; 2],
    up_hp: OnePole,
    env: f32,
    env_c: f32,
    flip: f32,
    flip2: f32,
    armed: bool,
    gate: f32,
    gate_c: f32,
    sub: Smooth,
    up: Smooth,
    dry: Smooth,
    sub2: Smooth,
}

pub fn octaver(rate: f32) -> Box<dyn Pedal> {
    let mut o = Octaver {
        rate,
        hp: Biquad::new(rate, Filter::HighPass { f: 22.0, q: 0.7 }),
        track: [Biquad::default(); 2],
        fc: 0.0,
        since: 0,
        sub_lp: [Biquad::new(rate, Filter::LowPass { f: 700.0, q: 0.7 }); 2],
        sub2_lp: [Biquad::new(rate, Filter::LowPass { f: 400.0, q: 0.7 }); 2],
        up_hp: OnePole::new(rate, 120.0),
        env: 0.0,
        env_c: coef_ms(rate, 20.0),
        flip: 1.0,
        flip2: 1.0,
        armed: true,
        gate: 0.0,
        gate_c: coef_ms(rate, 8.0),
        sub: Smooth::new(rate, 30.0, 0.6),
        up: Smooth::new(rate, 30.0, 0.0),
        dry: Smooth::new(rate, 30.0, 0.7),
        sub2: Smooth::new(rate, 30.0, 0.0),
    };
    o.track_at(TRACK_OPEN);
    Box::new(o)
}

impl Octaver {
    fn track_at(&mut self, f: f32) {
        self.fc = f;
        for b in &mut self.track {
            b.set(
                self.rate,
                Filter::LowPass {
                    f,
                    q: std::f32::consts::FRAC_1_SQRT_2,
                },
            );
        }
    }
}

impl Pedal for Octaver {
    fn process(&mut self, x: &mut [f32]) {
        let (shortest, longest) = ((self.rate / 450.0) as u32, (self.rate / 22.0) as u32);
        for v in x.iter_mut() {
            let t = {
                let t0 = self.hp.tick(*v);
                let t1 = self.track[0].tick(t0);
                self.track[1].tick(t1)
            };
            self.env = flush(self.env + (t.abs() - self.env) * self.env_c);
            self.since = self.since.saturating_add(1);
            // a flip-flop on the rising zero crossings, with hysteresis against noise
            let h = 0.08 * self.env;
            if self.armed && t > h {
                self.flip = -self.flip;
                if self.flip > 0.0 {
                    self.flip2 = -self.flip2;
                }
                self.armed = false;
                // the period found: the tracker's low-pass moves to just under it, which leaves the fundamental
                // on top however strong the 2nd harmonic was
                if (shortest..=longest).contains(&self.since) {
                    let f = (0.8 * self.rate / self.since as f32).clamp(TRACK_MIN, TRACK_OPEN);
                    if (f / self.fc - 1.0).abs() > 0.04 {
                        self.track_at(f);
                    }
                }
                self.since = 0;
            } else if t < -h {
                self.armed = true;
            }
            // nothing tracked for a while (a rest): open up for the next note
            if self.since > (self.rate * 0.15) as u32 && self.fc != TRACK_OPEN {
                self.track_at(TRACK_OPEN);
            }
            let open = if self.env > TRACK_FLOOR { 1.0 } else { 0.0 };
            self.gate = flush(self.gate + (open - self.gate) * self.gate_c);
            // (the tracker's low-pass takes the level down as it closes in: the octave's level follows the input's)
            let lift = (TRACK_OPEN / self.fc).sqrt().min(4.0);
            let sub = {
                let t0 = self.sub_lp[0].tick(t * self.flip);
                self.sub_lp[1].tick(t0)
            } * 1.6
                * lift
                * self.gate;
            let sub2 = {
                let t0 = self.sub2_lp[0].tick(t * self.flip2);
                self.sub2_lp[1].tick(t0)
            } * 1.6
                * lift
                * self.gate;
            let up = self.up_hp.hp(v.abs() * 1.2);
            *v = *v * self.dry.tick() + sub * self.sub.tick() + up * self.up.tick() + sub2 * self.sub2.tick();
        }
    }
    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        self.sub.target = unit10(v[0]);
        self.up.target = unit10(v[1]);
        self.dry.target = unit10(v[2]);
        self.sub2.target = unit10(v[3]);
    }
    fn reset(&mut self) {
        for b in self
            .track
            .iter_mut()
            .chain(self.sub_lp.iter_mut())
            .chain(self.sub2_lp.iter_mut())
        {
            b.reset();
        }
        self.hp.reset();
        self.track_at(TRACK_OPEN);
        self.since = 0;
        self.up_hp.reset();
        self.env = 0.0;
        self.gate = 0.0;
        self.sub.snap();
        self.up.snap();
        self.dry.snap();
        self.sub2.snap();
    }
}
