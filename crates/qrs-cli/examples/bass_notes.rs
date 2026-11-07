//! How a note test does on real bass (new in quickRS): each chart note against the recording, at the chart's time, as
//! charted and shifted off pitch (those should fail). Usage: bass_notes <song> <mono 16-bit wav> ...
use qrs_analysis::detect::{self, ATTACK_SKIP, BASS_WINDOW, BASS_WINDOW_MIN, NoteGroup};
use qrs_analysis::pitch::midi_to_hz;
use qrs_analysis::spectrum::Spectra;
use qrs_formats::package::Package;

fn read_wav(path: &str) -> (Vec<f32>, u32) {
    let b = std::fs::read(path).unwrap();
    let mut i = 12;
    let mut rate = 0;
    while i + 8 < b.len() {
        let id = &b[i..i + 4];
        let n = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"fmt " {
            rate = u32::from_le_bytes(b[i + 12..i + 16].try_into().unwrap());
        } else if id == b"data" {
            let end = (i + 8 + n).min(b.len());
            let x = b[i + 8..end]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                .collect();
            return (x, rate);
        }
        i += 8 + n + (n & 1);
    }
    panic!("no data in {path}");
}

/// The loudest local maximum within `tol` of `f`.
fn local(mag: &[f64], binhz: f64, f: f64, tol: f64) -> f64 {
    let lo = ((f * (1.0 - tol) / binhz) as usize).max(2);
    let hi = (f * (1.0 + tol) / binhz) as usize + 1;
    if hi + 1 >= mag.len() {
        return 0.0;
    }
    (lo..=hi)
        .filter(|&i| mag[i] >= mag[i - 1] && mag[i] >= mag[i + 1])
        .map(|i| mag[i])
        .fold(0.0, f64::max)
}

/// The harmonics of note `m`, loudness-compressed, weighted down the series.
fn salience(mag: &[f64], binhz: f64, m: f64) -> f64 {
    let f = midi_to_hz(m);
    let (mut s, mut w) = (0.0, 0.0);
    for h in 1..=24 {
        let fh = h as f64 * f;
        if fh > 1500.0 {
            break;
        }
        let k = 1.0 / (h as f64).sqrt();
        s += k * local(mag, binhz, fh, 0.012).sqrt();
        w += k;
    }
    if w > 0.0 { s / w } else { 0.0 }
}

/// How the note stands against every other within an octave (1: the best of them), octaves aside.
fn contest(mag: &[f64], binhz: f64, m: i32) -> f64 {
    let own = salience(mag, binhz, m as f64);
    let mut best: f64 = 1e-12;
    for d in 1..12 {
        best = best.max(salience(mag, binhz, (m + d) as f64));
        best = best.max(salience(mag, binhz, (m - d) as f64));
    }
    own / best
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let keys = Default::default();
    let shifts = [0, -1, 1, -2, 2, 5];
    let ks = [0.8, 0.9, 1.0, 1.1];
    let mut sp = Spectra::new();
    for pair in args.chunks(2) {
        let a = Package::open(std::path::Path::new(&pair[0]), &keys, None)
            .unwrap()
            .load(Some("bass"), 1.0)
            .unwrap();
        let (x, rate) = read_wav(&pair[1]);
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
        let x: Vec<f32> = x.iter().map(|v| v * 0.5 / peak).collect();
        let groups: Vec<NoteGroup> = detect::build_groups(&a);
        println!("{} {}", a.artist, a.title);
        let skip = (ATTACK_SKIP * rate as f64) as usize;
        let mut old = [0u32; 6];
        let mut new = [[0u32; 4]; 6];
        let mut why = [0u32; 3];
        let mut n = 0u32;
        for (i, g) in groups.iter().enumerate() {
            if g.muted || g.midis.len() != 1 {
                continue;
            }
            let low = g.midis[0] < 40;
            let w = if low { 0.17f64 } else { BASS_WINDOW };
            let room = groups
                .get(i + 1)
                .map_or(w, |nx| (nx.t - g.t - ATTACK_SKIP).max(BASS_WINDOW_MIN));
            let len = (w.min(room) * rate as f64) as usize;
            let at = (g.t * rate as f64) as usize + skip;
            if at + len > x.len() || at < len + skip {
                continue;
            }
            let (mag, binhz) = sp.spectrum(&x[at..at + len], rate);
            let (prev, _) = sp.spectrum(&x[at - skip - len..at - skip], rate);
            n += 1;
            for (s, &d) in shifts.iter().enumerate() {
                let mut t = g.clone();
                t.midis[0] += d;
                if d != 0 {
                    t.ringing.clear();
                    t.repeats.clear();
                }
                let v = detect::explain_bass(&mag, binhz, &t, Some(&prev));
                if v.ok {
                    old[s] += 1;
                }
                if d == 0 && !v.ok {
                    if v.present == 0 {
                        why[0] += 1;
                    }
                    if v.foreign > detect::FOREIGN_MAX {
                        why[1] += 1;
                    }
                    if v.present == 0 && v.foreign > detect::FOREIGN_MAX {
                        why[2] += 1;
                    }
                }
                let c = contest(&mag, binhz, t.midis[0]);
                for (j, &k) in ks.iter().enumerate() {
                    if c >= k {
                        new[s][j] += 1;
                    }
                }
            }
        }
        let pc = |v: u32| 100.0 * v as f64 / n.max(1) as f64;
        println!(
            "  {n} notes. today's test, right notes refused: {:.0}% for not present, {:.0}% for foreign peaks ({:.0}% both)",
            pc(why[0]),
            pc(why[1]),
            pc(why[2])
        );
        print!("  shift        ");
        for d in shifts {
            print!("{d:>6}");
        }
        print!("\n  today        ");
        for v in old {
            print!("{:>5.0}%", pc(v));
        }
        println!();
        for (j, k) in ks.iter().enumerate() {
            print!("  contest {k:<4} ");
            for r in &new {
                print!("{:>5.0}%", pc(r[j]));
            }
            println!();
        }
    }
}
