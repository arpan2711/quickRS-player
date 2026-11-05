//! Equalisers: graphic EQs (constant-Q peaking bands) and a four-band parametric. A moved slider glides (its band
//! is recomputed every 32 samples on the way), so dragging one never zips.

use super::dsp::{Biquad, Filter, Smooth, db_to_gain};
use super::{MAX_KNOBS, Pedal};

const STEP: usize = 32;

/// A bank of up to 8 filters whose gains glide.
struct Bank {
    rate: f32,
    kinds: [Kind; 8],
    freq: [f32; 8],
    q: [f32; 8],
    db: [f32; 8],
    target: [f32; 8],
    f: [Biquad; 8],
    n: usize,
    level: Smooth,
    count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Peak,
    LowShelf,
    HighShelf,
}

impl Bank {
    fn new(rate: f32, bands: &[(Kind, f32, f32)]) -> Bank {
        let mut b = Bank {
            rate,
            kinds: [Kind::Peak; 8],
            freq: [1000.0; 8],
            q: [1.0; 8],
            db: [0.0; 8],
            target: [0.0; 8],
            f: [Biquad::default(); 8],
            n: bands.len().min(8),
            level: Smooth::new(rate, 30.0, 1.0),
            count: 0,
        };
        for (i, &(k, f, q)) in bands.iter().take(8).enumerate() {
            b.kinds[i] = k;
            b.freq[i] = f;
            b.q[i] = q;
            b.design(i);
        }
        b
    }

    fn design(&mut self, i: usize) {
        let (f, q, db) = (self.freq[i], self.q[i], self.db[i]);
        let filt = match self.kinds[i] {
            Kind::Peak => Filter::Peak { f, q, db },
            Kind::LowShelf => Filter::LowShelf { f, db },
            Kind::HighShelf => Filter::HighShelf { f, db },
        };
        self.f[i].set(self.rate, filt);
    }

    fn glide(&mut self) {
        for i in 0..self.n {
            let d = self.target[i] - self.db[i];
            if d.abs() > 1e-3 {
                self.db[i] += if d.abs() < 0.05 { d } else { d * 0.25 };
                self.design(i);
            }
        }
    }

    fn process(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            if self.count == 0 {
                self.glide();
                self.count = STEP;
            }
            self.count -= 1;
            let mut s = *v;
            for b in self.f.iter_mut().take(self.n) {
                s = b.tick(s);
            }
            *v = s * self.level.tick();
        }
    }

    fn reset(&mut self) {
        for i in 0..self.n {
            self.db[i] = self.target[i];
            self.design(i);
            self.f[i].reset();
        }
        self.level.snap();
    }
}

struct Graphic {
    bank: Bank,
    bands: usize,
}

impl Pedal for Graphic {
    fn process(&mut self, x: &mut [f32]) {
        self.bank.process(x);
    }
    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        self.bank.target[..self.bands].copy_from_slice(&v[..self.bands]);
        self.bank.level.target = db_to_gain(v[self.bands]);
    }
    fn reset(&mut self) {
        self.bank.reset();
    }
}

fn graphic(rate: f32, freqs: &[f32], q: f32) -> Box<dyn Pedal> {
    let bands: Vec<(Kind, f32, f32)> = freqs.iter().map(|&f| (Kind::Peak, f, q)).collect();
    Box::new(Graphic {
        bank: Bank::new(rate, &bands),
        bands: freqs.len(),
    })
}

/// RS's 5-band EQ: bands two octaves apart, so wider.
pub fn graphic5(rate: f32) -> Box<dyn Pedal> {
    graphic(rate, &[63.0, 250.0, 750.0, 2200.0, 5700.0], 0.8)
}

/// Octave bands.
pub fn graphic8(rate: f32) -> Box<dyn Pedal> {
    graphic(
        rate,
        &[50.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0],
        1.4,
    )
}

pub fn bass_graphic8(rate: f32) -> Box<dyn Pedal> {
    graphic(
        rate,
        &[30.0, 75.0, 185.0, 460.0, 1100.0, 2700.0, 6800.0, 16000.0],
        1.1,
    )
}

struct Studio {
    bank: Bank,
}

pub fn studio(rate: f32) -> Box<dyn Pedal> {
    Box::new(Studio {
        bank: Bank::new(
            rate,
            &[
                (Kind::LowShelf, 120.0, 0.7),
                (Kind::Peak, 500.0, 0.9),
                (Kind::Peak, 2000.0, 0.9),
                (Kind::HighShelf, 5000.0, 0.7),
            ],
        ),
    })
}

impl Pedal for Studio {
    fn process(&mut self, x: &mut [f32]) {
        self.bank.process(x);
    }
    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        // bass, bass freq, lo-mid, lo freq, hi-mid, hi freq, treble, treble freq
        let b = &mut self.bank;
        let moved = |old: f32, new: f32| (old - new).abs() > 0.01;
        for (i, (gain, freq)) in [(v[0], v[1]), (v[2], v[3]), (v[4], v[5]), (v[6], v[7])]
            .into_iter()
            .enumerate()
        {
            b.target[i] = gain;
            if moved(b.freq[i], freq) {
                b.freq[i] = freq;
                b.design(i);
            }
        }
    }
    fn reset(&mut self) {
        self.bank.reset();
    }
}
