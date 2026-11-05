//! Delays. All are "wet only": the board passes the dry guitar by and adds what they return, so switching one off
//! stops new echoes and lets the old ones fade.
//!
//! - Analog: a bucket-brigade line: repeats darker the longer the delay (the chip's clock slows), a soft limit
//!   in the loop so high Repeats bloom instead of exploding, a slow wobble.
//! - Tape: wow and flutter on the read head, the tape's head bump, top loss and saturation in the loop; Repeats
//!   past 9 runs away into self-oscillation (bounded by the saturation). Heads (D45): any of the three playback
//!   heads, at 1x, 2x and 3x the Time as on the RE-201; the repeats feed back from all the heads that play.
//! - Digital: clean repeats with a tone control in the loop.
//! - Mod: a digital delay whose time is chorused.
//!
//! Changing Time glides the read head (a tape-like pitch bend) instead of jumping.
//!
//! Sync (P37.2): Off, or a note value at the song's tempo (`board::tempo`, set by the app when a song opens): the
//! Time knob is then ignored, and a tempo change glides the repeats there.

use super::catalog::{HEAD_BITS, SYNC_BEATS};
use super::dsp::{Biquad, Filter, Lfo, Line, OnePole, Smooth, coef_ms, flush, sweep, unit10};
use super::{MAX_KNOBS, Pedal};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Voice {
    Analog,
    Tape,
    Digital,
    Mod,
}

struct Delay {
    voice: Voice,
    rate: f32,
    line: Line,
    /// The read position in samples, gliding to `target`.
    d: f32,
    target: f32,
    glide: f32,
    fb: f32,
    wet: Smooth,
    lp: Biquad,
    hp: OnePole,
    bump: Biquad,
    lfo: Lfo,
    lfo2: Lfo,
    mod_depth: f32,
    flutter: f32,
    sat: f32,
    last: f32,
    /// Tape: the playback heads that play (bit k: head k + 1, at k + 1 times the Time).
    heads: u8,
    /// Sync: the repeat in quarter notes at the song's tempo (0: the Time knob).
    beats: f32,
}

fn new(voice: Voice, rate: f32, max_s: f32) -> Box<dyn Pedal> {
    Box::new(Delay {
        voice,
        rate,
        line: Line::new((rate * max_s) as usize + 64),
        d: 0.3 * rate,
        target: 0.3 * rate,
        glide: coef_ms(rate, 180.0),
        fb: 0.3,
        wet: Smooth::new(rate, 30.0, 0.3),
        lp: Biquad::new(rate, Filter::LowPass { f: 4000.0, q: 0.7 }),
        hp: OnePole::new(rate, 80.0),
        bump: Biquad::new(
            rate,
            Filter::Peak {
                f: 100.0,
                q: 0.8,
                db: 0.0,
            },
        ),
        lfo: Lfo::default(),
        lfo2: Lfo::default(),
        mod_depth: 0.0,
        flutter: 0.0,
        sat: 1.0,
        last: 0.0,
        heads: 1,
        beats: 0.0,
    })
}

pub fn analog(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Analog, rate, 0.62)
}
pub fn tape(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Tape, rate, 2.48)
}
pub fn digital(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Digital, rate, 1.02)
}
pub fn modulated(rate: f32) -> Box<dyn Pedal> {
    new(Voice::Mod, rate, 1.03)
}

impl Delay {
    /// Sync on: the repeat time from the song's tempo (it glides there when the tempo changes).
    fn sync(&mut self) {
        if self.beats > 0.0 {
            let bpm = super::tempo();
            if bpm > 0.0 {
                let max = (self.line.len() - 8) as f32 / (if self.heads > 1 { 3.0 } else { 1.0 });
                self.target = (60.0 / bpm * self.beats * self.rate).clamp(2.0, max);
            }
        }
    }
}

impl Pedal for Delay {
    fn process(&mut self, x: &mut [f32]) {
        let r = self.rate;
        self.sync();
        for v in x.iter_mut() {
            self.d += (self.target - self.d) * self.glide;
            let p = self.lfo.step();
            let mut d = self.d + self.mod_depth * Lfo::sine(p);
            if self.voice == Voice::Tape {
                // flutter: a faster, smaller wobble on top of the wow
                let q = self.lfo2.step();
                d += self.flutter * Lfo::sine(q);
            }
            let max = (self.line.len() - 8) as f32;
            let (echo, fed) = if self.heads == 1 {
                let e = self.line.cubic(d.clamp(2.0, max));
                (e, e)
            } else {
                // each head further along the tape: the same wobble, k times the distance
                let (mut sum, mut n) = (0.0, 0.0f32);
                for k in 0..3 {
                    if self.heads & (1 << k) != 0 {
                        sum += self.line.cubic((d * (k + 1) as f32).clamp(2.0, max));
                        n += 1.0;
                    }
                }
                (sum / n.sqrt(), sum / n)
            };
            let mut back = self.hp.hp(self.lp.tick(fed));
            match self.voice {
                Voice::Analog => back = (back * self.sat).tanh() / self.sat,
                Voice::Tape => {
                    back = self.bump.tick(back);
                    // tape saturates a little lopsided
                    let s = back * self.sat + 0.05;
                    back = (s.tanh() - 0.05f32.tanh()) / self.sat;
                }
                _ => {}
            }
            self.last = flush(back);
            self.line.push(*v + back * self.fb);
            *v = echo * self.wet.tick();
        }
        let _ = r;
    }

    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        let r = self.rate;
        let time = v[0] * 1e-3;
        let repeats = unit10(v[1]);
        self.target = (time * r).max(2.0);
        let sync = match self.voice {
            Voice::Analog | Voice::Digital => v[4],
            Voice::Tape => v[5],
            Voice::Mod => v[5],
        };
        self.beats = SYNC_BEATS[(sync.round().max(0.0) as usize).min(SYNC_BEATS.len() - 1)];
        self.wet.target = (v[2] / 100.0).clamp(0.0, 1.0) * 1.3;
        match self.voice {
            Voice::Analog => {
                // the longer the delay, the slower the bucket-brigade clock and the darker the repeats
                let short = 1.0 - ((time - 0.02) / 0.58).clamp(0.0, 1.0);
                self.lp.set(
                    r,
                    Filter::LowPass {
                        f: 2200.0 + 2600.0 * short,
                        q: 0.6,
                    },
                );
                self.hp.set(r, 90.0);
                self.fb = 0.98 * repeats;
                self.sat = 2.0;
                self.lfo.set(r, 0.55);
                self.mod_depth = unit10(v[3]) * 0.0006 * r;
            }
            Voice::Tape => {
                self.lp.set(r, Filter::LowPass { f: 4200.0, q: 0.6 });
                self.hp.set(r, 120.0);
                self.bump.set(
                    r,
                    Filter::Peak {
                        f: 110.0,
                        q: 0.8,
                        db: 2.5,
                    },
                );
                // past 9, the loop gain goes over one and the saturation holds it
                self.fb = 1.08 * repeats;
                self.sat = 1.6;
                let wow = unit10(v[3]);
                self.lfo.set(r, 0.45);
                self.lfo2.set(r, 7.3);
                self.mod_depth = wow * 0.0016 * r;
                self.flutter = wow * 0.00008 * r;
                self.heads = HEAD_BITS[(v[4].round().max(0.0) as usize).min(6)];
            }
            Voice::Digital => {
                self.lp.set(
                    r,
                    Filter::LowPass {
                        f: sweep(1800.0, 16000.0, unit10(v[3])),
                        q: 0.6,
                    },
                );
                self.hp.set(r, 40.0);
                self.fb = 0.92 * repeats;
                self.mod_depth = 0.0;
            }
            Voice::Mod => {
                self.lp.set(r, Filter::LowPass { f: 6000.0, q: 0.6 });
                self.hp.set(r, 60.0);
                self.fb = 0.92 * repeats;
                self.lfo.set(r, sweep(0.1, 3.0, unit10(v[3])));
                self.mod_depth = unit10(v[4]) * 0.0022 * r;
            }
        }
    }

    fn reset(&mut self) {
        self.line.clear();
        self.lp.reset();
        self.hp.reset();
        self.bump.reset();
        self.sync();
        self.d = self.target;
        self.last = 0.0;
        self.wet.snap();
    }

    fn wet_only(&self) -> bool {
        true
    }
}
