//! Compare our decode of a song with a vgmstream-decoded WAV of it (tools/parity).
//!   cargo run --release -p qrs-audio --example decode_compare -- KEYS.json SONG.psarc[:songkey] REF.wav [preview]
use qrs_audio::decode::{decode, read_wav_i16};
use qrs_formats::{Package, load_keys, song_audio::song_audio};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let keys = load_keys(Some(std::path::Path::new(&a[1])), &[]).unwrap();
    let (path, key) = match a[2].rsplit_once('|') {
        Some((p, k)) => (p.to_string(), Some(k.to_string())),
        None => (a[2].clone(), None),
    };
    let preview = a.get(4).map(|s| s == "preview").unwrap_or(false);
    let t0 = std::time::Instant::now();
    let mut pkg = Package::open(&path, &keys, key.as_deref()).unwrap();
    let enc = song_audio(&mut pkg, preview).unwrap().unwrap();
    let t1 = t0.elapsed();
    let pcm = decode(&enc).unwrap();
    let t2 = t0.elapsed();
    let (rate, ch, reference) = read_wav_i16(&std::fs::read(&a[3]).unwrap()).unwrap();
    println!(
        "ours: {} Hz x{} {} frames ({:.2}s) | ref: {} Hz x{} {} frames | wem+ogg {:?}, decode {:?}",
        pcm.rate,
        pcm.channels,
        pcm.frames(),
        pcm.duration(),
        rate,
        ch,
        reference.len() / ch as usize,
        t1,
        t2 - t1
    );
    let n = pcm.samples.len().min(reference.len());
    for (name, conv) in [
        (
            "x32768 round",
            (|s: f32| (s * 32768.0).round().clamp(-32768.0, 32767.0) as i16) as fn(f32) -> i16,
        ),
        ("x32767 round", |s: f32| {
            (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16
        }),
        ("x32768 trunc", |s: f32| {
            (s * 32768.0).clamp(-32768.0, 32767.0) as i16
        }),
        ("x32768 floor+.5", |s: f32| {
            (s * 32768.0 + 0.5).floor().clamp(-32768.0, 32767.0) as i16
        }),
        ("x32767 trunc", |s: f32| {
            (s * 32767.0).clamp(-32768.0, 32767.0) as i16
        }),
        ("x32767 floor", |s: f32| {
            (s * 32767.0).floor().clamp(-32768.0, 32767.0) as i16
        }),
        ("x32768 floor", |s: f32| {
            (s * 32768.0).floor().clamp(-32768.0, 32767.0) as i16
        }),
        ("x32767.5 trunc", |s: f32| {
            (s * 32767.5).clamp(-32768.0, 32767.0) as i16
        }),
    ] {
        let (mut exact, mut within1, mut worst) = (0usize, 0usize, 0i32);
        for i in 0..n {
            let d = (conv(pcm.samples[i]) as i32 - reference[i] as i32).abs();
            if d == 0 {
                exact += 1
            }
            if d <= 1 {
                within1 += 1
            }
            worst = worst.max(d);
        }
        println!(
            "  {name:<16} exact {:.4}%  within 1 LSB {:.4}%  worst {worst}",
            100.0 * exact as f64 / n as f64,
            100.0 * within1 as f64 / n as f64
        );
    }
}
