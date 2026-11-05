//! What a block does, as a picture for the big view: a frequency response (amps' tone stacks, cabs, EQs, filters),
//! a transfer curve (drives and fuzzes: output against input), a compressor's gain curve, echo taps (delays), a
//! tail's decay (reverbs) or the movement over two seconds (modulation). Worked out from the same numbers the
//! models run on.

use super::amp::{self, Stack};
use super::dsp::{Biquad, Curve, Filter, audio_taper, sweep, unit10};
use super::{Block, Face, Kind, Unit, cab};

#[derive(Clone, Debug)]
pub enum Picture {
    /// (Hz, dB) from 20 Hz to 20 kHz.
    Response(Vec<(f32, f32)>),
    /// (input, output), both -1..1.
    Transfer(Vec<(f32, f32)>),
    /// (input dB, output dB), -60..0.
    Dynamics(Vec<(f32, f32)>),
    /// (seconds after the note, level 0..1) and the span shown.
    Echoes(Vec<(f32, f32)>, f32),
    /// (seconds, level 0..1).
    Decay(Vec<(f32, f32)>, f32),
    /// (seconds, position 0..1) over two seconds.
    Movement(Vec<(f32, f32)>),
}

const RATE: f32 = 48000.0;

fn freqs() -> impl Iterator<Item = f32> {
    (0..=120).map(|i| 20.0 * 1000f32.powf(i as f32 / 120.0))
}

fn response(filters: &[Filter], trim_db: f32) -> Picture {
    let bq: Vec<Biquad> = filters.iter().map(|f| Biquad::new(RATE, *f)).collect();
    Picture::Response(
        freqs()
            .map(|f| {
                let g: f32 = bq.iter().map(|b| b.magnitude(f, RATE)).product();
                (f, 20.0 * g.max(1e-6).log10() + trim_db)
            })
            .collect(),
    )
}

fn knob(b: &Block, key: &str, default: f32) -> f32 {
    b.get(key).unwrap_or(default)
}

/// The stack and its knobs for an amp (None for the tweed's single tone knob).
fn amp_stack(b: &Block) -> Option<(Stack, f32, f32, f32)> {
    let t = unit10(knob(b, "treble", 5.0));
    let m = unit10(knob(b, "mid", 5.0));
    let l = unit10(knob(b, "bass", 5.0));
    match b.def().id {
        "amp_clean" => Some((amp::TWIN, t, m, l)),
        "amp_chime" => Some((amp::AC30, t, 0.5, l)),
        "amp_crunch" => Some((amp::JTM45, t, m, l)),
        "amp_lead" => Some((amp::JCM800, t, m, l)),
        "amp_modern" | "amp_cali" => Some((amp::MESA, t, m, l)),
        "amp_snake" | "amp_tangerine" => Some((amp::JCM800, t, m, l)),
        "amp_surrey" => Some((amp::JTM45, t, m, l)),
        "amp_bass" => Some((amp::AMPEG, t, 0.5, l)),
        _ => None,
    }
}

pub fn picture(b: &Block) -> Picture {
    let def = b.def();
    // (a capture or IR: measured from its file; flat until it's loaded)
    if def.file {
        return super::nam::picture(b.asset).unwrap_or_else(|| match def.kind {
            Kind::Amp | Kind::Drive => Picture::Transfer(vec![(-1.0, -1.0), (1.0, 1.0)]),
            _ => Picture::Response(freqs().map(|f| (f, 0.0)).collect()),
        });
    }
    let v = &b.v;
    match def.kind {
        Kind::Amp => match amp_stack(b) {
            Some((stack, t, m, l)) => {
                let pres = unit10(knob(b, "presence", 0.0)) * 8.0;
                let depth = unit10(knob(b, "depth", 0.0)) * 10.0;
                let shelf = Biquad::new(RATE, Filter::HighShelf { f: 3500.0, db: pres });
                let bump = Biquad::new(
                    RATE,
                    Filter::Peak {
                        f: 90.0,
                        q: 0.8,
                        db: depth,
                    },
                );
                let mid_bq = (def.id == "amp_bass").then(|| {
                    Biquad::new(
                        RATE,
                        Filter::Peak {
                            f: knob(b, "freq", 800.0),
                            q: 0.9,
                            db: (unit10(knob(b, "mid", 5.0)) - 0.5) * 20.0,
                        },
                    )
                });
                let pts: Vec<(f32, f32)> = freqs()
                    .map(|f| {
                        let mut db = stack.response_db(f as f64, t as f64, m as f64, l as f64) as f32;
                        db += 20.0 * (shelf.magnitude(f, RATE) * bump.magnitude(f, RATE)).log10();
                        if let Some(q) = &mid_bq {
                            db += 20.0 * q.magnitude(f, RATE).log10();
                        }
                        (f, db)
                    })
                    .collect();
                // shown around 0 dB: the stack's loss is made up after it
                let top = pts.iter().fold(f32::MIN, |a, p| a.max(p.1));
                Picture::Response(pts.into_iter().map(|(f, d)| (f, d - top + 3.0)).collect())
            }
            None => {
                let tone = unit10(knob(b, "tone", 6.0));
                response(
                    &[
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
                        Filter::HighPass { f: 40.0, q: 0.6 },
                    ],
                    0.0,
                )
            }
        },
        Kind::Cab => Picture::Response(
            freqs()
                .map(|f| (f, cab::response_db(def.id, v, RATE, f)))
                .collect(),
        ),
        Kind::Eq => {
            let fs: Vec<Filter> = match def.id {
                "studio_eq" => vec![
                    Filter::LowShelf { f: v[1], db: v[0] },
                    Filter::Peak {
                        f: v[3],
                        q: 0.9,
                        db: v[2],
                    },
                    Filter::Peak {
                        f: v[5],
                        q: 0.9,
                        db: v[4],
                    },
                    Filter::HighShelf { f: v[7], db: v[6] },
                ],
                _ => {
                    let (fr, q): (&[f32], f32) = match def.id {
                        "eq5" => (&[63.0, 250.0, 750.0, 2200.0, 5700.0], 0.8),
                        "eq8" => (&[50.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0], 1.4),
                        _ => (&[30.0, 75.0, 185.0, 460.0, 1100.0, 2700.0, 6800.0, 16000.0], 1.1),
                    };
                    fr.iter()
                        .enumerate()
                        .map(|(i, &f)| Filter::Peak { f, q, db: v[i] })
                        .collect()
                }
            };
            let level = if def.id == "studio_eq" {
                0.0
            } else {
                v[def.knobs.len() - 1]
            };
            response(&fs, level)
        }
        Kind::Filter => match def.id {
            "wah" | "env_filter" => {
                // where the filter sits now: the pedal's position (or the middle of an envelope's sweep)
                let (fc, q) = if def.id == "wah" {
                    let p = unit10(v[0]);
                    (sweep(420.0, 2300.0, p), 3.0 + 3.5 * p)
                } else {
                    (900.0, 1.0 + 9.0 * unit10(v[1]))
                };
                let pts = freqs()
                    .map(|f| {
                        let x = f / fc;
                        let den = ((1.0 - x * x).powi(2) + (x / q).powi(2)).sqrt();
                        let (lp, bp) = (1.0 / den, (x / q) / den);
                        let g = if def.id == "wah" {
                            0.35 * lp + 2.5 * bp
                        } else {
                            3.5 * bp + 0.45 * lp
                        };
                        (f, 20.0 * g.max(1e-6).log10())
                    })
                    .collect();
                Picture::Response(pts)
            }
            "lofi" => {
                let fs = match v[0].round() as i32 {
                    0 => vec![
                        Filter::HighPass { f: 250.0, q: 0.8 },
                        Filter::LowPass { f: 3500.0, q: 0.9 },
                        Filter::Peak {
                            f: 1200.0,
                            q: 1.2,
                            db: 4.0,
                        },
                    ],
                    1 => vec![
                        Filter::HighPass { f: 500.0, q: 1.0 },
                        Filter::LowPass { f: 2700.0, q: 1.2 },
                        Filter::Peak {
                            f: 1500.0,
                            q: 1.5,
                            db: 6.0,
                        },
                        Filter::HighPass { f: 400.0, q: 0.7 },
                    ],
                    _ => vec![
                        Filter::LowPass { f: 9000.0, q: 0.7 },
                        Filter::HighPass { f: 40.0, q: 0.7 },
                    ],
                };
                response(&fs, 0.0)
            }
            _ => {
                let (body, top) = (unit10(v[0]), unit10(v[1]));
                response(
                    &[
                        Filter::HighPass { f: 60.0, q: 0.6 },
                        Filter::Peak {
                            f: 115.0,
                            q: 1.3,
                            db: 9.0 * body,
                        },
                        Filter::Peak {
                            f: 220.0,
                            q: 1.0,
                            db: 2.0 * body,
                        },
                        Filter::Peak {
                            f: 420.0,
                            q: 0.9,
                            db: -5.0,
                        },
                        Filter::Peak {
                            f: 1300.0,
                            q: 1.4,
                            db: -4.0,
                        },
                        Filter::HighShelf {
                            f: 3800.0,
                            db: 2.0 + 9.0 * top,
                        },
                    ],
                    0.0,
                )
            }
        },
        Kind::Dynamics if def.id != "boost" && def.id != "gate" => {
            // the static gain curve, as the compressor computes it
            let (thr, ratio) = match def.id {
                "studio_comp" => (v[0], v[1].max(1.0)),
                "bass_comp" => (-14.0 - 22.0 * unit10(v[0]), 2.0 + 8.0 * unit10(v[0])),
                _ => (-10.0 - 30.0 * unit10(v[0]), 2.0 + 14.0 * unit10(v[0])),
            };
            let knee = 6.0;
            let pts = (0..=60)
                .map(|i| {
                    let x = -60.0 + i as f32;
                    let over = x - thr;
                    let slope = 1.0 - 1.0 / ratio;
                    let red = if 2.0 * over < -knee {
                        0.0
                    } else if 2.0 * over.abs() <= knee {
                        slope * (over + knee / 2.0).powi(2) / (2.0 * knee)
                    } else {
                        slope * over
                    };
                    (x, x - red)
                })
                .collect();
            Picture::Dynamics(pts)
        }
        Kind::Dynamics => {
            // the gate's (or the boost's) level in against level out
            let pts = (0..=60)
                .map(|i| {
                    let x = -60.0 + i as f32;
                    let y = if def.id == "gate" {
                        if x < v[0] { x - 30.0 } else { x }
                    } else {
                        (x + v[0]).min(-1.0)
                    };
                    (x, y.max(-60.0))
                })
                .collect();
            Picture::Dynamics(pts)
        }
        Kind::Drive | Kind::Fuzz => {
            // output against input through the clipper: the gain knob steepens it, the curve rounds or squares it
            let first = def
                .knobs
                .iter()
                .position(|k| matches!(k.unit, Unit::Dial))
                .unwrap_or(0);
            let mut g = 1.0 + 60.0 * audio_taper(unit10(v[first]));
            let mut curve = match def.id {
                "orange" | "big_pi" | "green" | "metal" => Curve::Tanh,
                "rodent" => Curve::Hard,
                "gold" | "germanium" => Curve::Soft,
                _ => Curve::Asym(0.5),
            };
            // other diodes inside: their curve, and their knee against silicon's
            if let Some(i) = def.knob_index("diodes")
                && (v[i] - def.knobs[i].def).abs() > 0.5
            {
                let (c, knee) = match v[i].round() as usize {
                    0 => (Curve::Tanh, 1.0),
                    1 => (Curve::Asym(0.6), 1.0),
                    2 => (Curve::Hard, 0.39),
                    _ => (Curve::Soft, 1.5),
                };
                curve = c;
                g *= knee;
            }
            let bias = if def.id == "round_fuzz" { 0.25 } else { 0.0 };
            let norm = curve
                .f((g + bias) as f64)
                .abs()
                .max(curve.f((-g + bias) as f64).abs()) as f32;
            let off = curve.f(bias as f64) as f32;
            let pts = (0..=100)
                .map(|i| {
                    let x = -1.0 + 2.0 * i as f32 / 100.0;
                    let y = (curve.f((x * g + bias) as f64) as f32 - off) / norm.max(1e-6);
                    (x, y.clamp(-1.0, 1.0))
                })
                .collect();
            Picture::Transfer(pts)
        }
        Kind::Delay => {
            let time = v[0] / 1000.0;
            let fb = unit10(v[1]) * if def.id == "tape_echo" { 1.08 } else { 0.92 };
            let mix = (v[2] / 100.0).clamp(0.0, 1.0);
            let span = (time * 8.0).clamp(1.0, 4.0);
            let mut taps = vec![(0.0, 1.0)];
            let heads = match def.knob_index("heads") {
                Some(i) => super::catalog::HEAD_BITS[(v[i].round().max(0.0) as usize).min(6)],
                None => 1,
            };
            if heads != 1 {
                // the tape's heads at 1x, 2x, 3x the Time: what goes on the tape at each step of Time, what the
                // heads play back (and feed back, averaged)
                let n = heads.count_ones() as f32;
                let steps = (span / time.max(1e-3)) as usize;
                let mut tape = vec![0.0f32; steps + 1];
                tape[0] = 1.0;
                for m in 1..=steps {
                    let played: f32 = (0..3)
                        .filter(|k| heads & (1 << k) != 0 && m > *k)
                        .map(|k| tape[m - k - 1])
                        .sum();
                    tape[m] = played / n * fb.min(0.98);
                    let g = played / n.sqrt() * mix * 1.3;
                    if g > 0.01 {
                        taps.push((m as f32 * time, g.min(1.0)));
                    }
                }
                return Picture::Echoes(taps, span);
            }
            let mut g = mix * 1.3;
            let mut t = time;
            while t <= span && g > 0.01 {
                taps.push((t, g.min(1.0)));
                g *= fb.min(0.98);
                t += time;
            }
            Picture::Echoes(taps, span)
        }
        Kind::Reverb => {
            let rt = match def.id {
                "spring" => 1.6 + 1.6 * unit10(v[0]),
                "room" => 0.25 + 1.4 * unit10(v[0]),
                _ => v[0],
            };
            let span = (rt * 1.3).clamp(1.0, 8.0);
            let pre = if def.face == Face::Rack || def.id == "hall" {
                v[2] / 1000.0
            } else {
                0.0
            };
            let pts = (0..=120)
                .map(|i| {
                    let t = span * i as f32 / 120.0;
                    let y = if t < pre {
                        0.0
                    } else {
                        10f32.powf(-3.0 * (t - pre) / rt)
                    };
                    (t, y)
                })
                .collect();
            Picture::Decay(pts, span)
        }
        Kind::Modulation => {
            let rate_hz = match def.id {
                "chorus" => sweep(0.12, 6.0, unit10(v[0])),
                "flanger" => sweep(0.05, 4.0, unit10(v[0])),
                "phaser" => sweep(0.08, 8.0, unit10(v[0])),
                "tremolo" => sweep(1.0, 12.0, unit10(v[0])),
                "vibe" => sweep(0.6, 9.0, unit10(v[0])),
                "rotary" => {
                    if v[0] > 0.5 {
                        6.8
                    } else {
                        0.8
                    }
                }
                _ => 0.0,
            };
            let depth = match def.id {
                "rotary" => unit10(v[2]),
                "octaver" => unit10(v[0]),
                _ => unit10(v[1]),
            };
            let pts = (0..=200)
                .map(|i| {
                    let t = 2.0 * i as f32 / 200.0;
                    let ph = (t * rate_hz).fract();
                    let w = match (def.id, v.get(2).copied().unwrap_or(0.0).round() as i32) {
                        ("tremolo", 1) | ("chorus", _) | ("flanger", _) => 1.0 - 4.0 * (ph - 0.5).abs(),
                        ("tremolo", 2) => (4.0 * (std::f32::consts::TAU * ph).sin()).tanh(),
                        ("octaver", _) => 0.0,
                        _ => (std::f32::consts::TAU * ph).sin(),
                    };
                    (t, 0.5 + 0.45 * depth * w)
                })
                .collect();
            Picture::Movement(pts)
        }
    }
}

/// What the picture is of, in a few words, for its title.
pub fn caption(b: &Block) -> (&'static str, &'static str) {
    match (b.def().file, b.def().kind) {
        (true, Kind::Amp | Kind::Drive) => {
            return (
                "BREAKUP",
                "the capture's output over one cycle of a low note: squarer breaks up more",
            );
        }
        (true, _) => return ("FREQUENCY RESPONSE", "the impulse response, measured"),
        _ => {}
    }
    match b.def().kind {
        Kind::Amp => (
            "TONE STACK",
            "the passive treble, mid and bass network after the preamp",
        ),
        Kind::Cab => ("SPEAKER AND MIC", "the cab's response where the mic is"),
        Kind::Eq => ("EQ", "dB across the frequencies"),
        Kind::Filter => ("FILTER", "where the filter is right now"),
        Kind::Dynamics => (
            "LEVEL IN, LEVEL OUT",
            "dB: above the threshold the level rises more slowly",
        ),
        Kind::Drive | Kind::Fuzz => ("CLIPPING", "output against input: the more gain, the squarer"),
        Kind::Delay => ("ECHOES", "each repeat after a note"),
        Kind::Reverb => ("DECAY", "the tail fading after a note stops"),
        Kind::Modulation => ("MOVEMENT", "what the effect sweeps, over two seconds"),
    }
}
