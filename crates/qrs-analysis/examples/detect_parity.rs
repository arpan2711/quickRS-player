//! Writes synthetic detection cases and our answers, for tools/parity/detect_ref.py to check against detect.py.
//!   cargo run --release -p qrs-analysis --example detect_parity -- OUT_DIR
use std::collections::HashSet;
use std::io::Write;

use qrs_analysis::Spectra;
use qrs_analysis::detect::{foreign_share, notes_present};
use qrs_analysis::pitch::{midi_to_hz, pitch};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn int(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() * (hi - lo + 1) as f64) as i32
    }
}

fn note(x: &mut [f32], rate: u32, midi: f64, amp: f64, t0: f64, decay: f64, rng: &mut Rng) {
    let f = midi_to_hz(midi);
    let harm: Vec<f64> = (1..=6)
        .map(|h| (0.9 / h as f64) * (0.6 + 0.8 * rng.next()))
        .collect();
    for (i, s) in x.iter_mut().enumerate() {
        let t = i as f64 / rate as f64 + t0;
        let mut v = 0.0;
        for (k, a) in harm.iter().enumerate() {
            v += a * (std::f64::consts::TAU * f * (k + 1) as f64 * t).sin();
        }
        *s += (amp * v * (-t * decay).exp()) as f32;
    }
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    let rate = 48000u32;
    let mut rng = Rng(12345);
    let mut sp = Spectra::new();
    let mut cases = Vec::new();
    for case in 0..300 {
        let n = if case % 5 == 0 { 8160 } else { 4080 };
        let chord: Vec<i32> = (0..rng.int(1, 4)).map(|_| rng.int(28, 76)).collect();
        let wrong = case % 4 == 1;
        let played: Vec<i32> = if wrong {
            chord
                .iter()
                .map(|m| m + if rng.next() < 0.5 { 1 } else { 2 })
                .collect()
        } else {
            chord.clone()
        };
        let mut x = vec![0.0f32; n];
        for &m in &played {
            let detune = (rng.next() - 0.5) * 0.2;
            let (amp, decay) = (0.15 + 0.2 * rng.next(), 2.0 + 4.0 * rng.next());
            note(&mut x, rate, m as f64 + detune, amp, 0.0, decay, &mut rng);
        }
        // a previous note ringing in, sometimes
        let ringing: Vec<i32> = if case % 3 == 0 {
            vec![rng.int(40, 70)]
        } else {
            vec![]
        };
        let mut before = vec![0.0f32; n];
        for &m in &ringing {
            note(&mut x, rate, m as f64, 0.1, 0.3, 3.0, &mut rng);
            note(&mut before, rate, m as f64, 0.1, 0.2, 3.0, &mut rng);
        }
        for v in x.iter_mut().chain(before.iter_mut()) {
            *v += ((rng.next() - 0.5) * 0.004) as f32;
        }
        let repeats: HashSet<i32> = if case % 7 == 0 {
            chord.iter().take(1).copied().collect()
        } else {
            HashSet::new()
        };
        let np = notes_present(&mut sp, &x, rate, &chord, Some(&before), &repeats);
        let np0 = notes_present(&mut sp, &x, rate, &chord, None, &repeats);
        let fs = foreign_share(&mut sp, &x, rate, &chord, &ringing);
        let (f, clarity) = pitch(&mut sp, &x, rate);
        let write = |name: &str, v: &[f32]| {
            let mut fh = std::fs::File::create(out.join(name)).unwrap();
            for s in v {
                fh.write_all(&s.to_le_bytes()).unwrap();
            }
        };
        write(&format!("x{case}.f32"), &x);
        write(&format!("b{case}.f32"), &before);
        cases.push(serde_json::json!({
            "id": case, "rate": rate, "midis": chord, "repeats": repeats.iter().collect::<Vec<_>>(), "ringing": ringing,
            "rust": {"present": np, "present_nobefore": np0, "foreign": fs, "pitch": f, "clarity": clarity}
        }));
    }
    std::fs::write(out.join("cases.json"), serde_json::to_string(&cases).unwrap()).unwrap();
    println!("wrote {} cases to {}", cases.len(), out.display());
}
