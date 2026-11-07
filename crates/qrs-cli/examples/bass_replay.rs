//! Play a song's bass chart "perfectly" with a synthetic bass through the live pluck detector and the scorer (new in
//! quickRS): what a right performance scores, and why the rest is lost. Usage: bass_replay <song> [jitter ms]
use qrs_analysis::detect::{self, Judgement, Live, Pluck, SampleSource, Scorer};
use qrs_analysis::instrument::Instrument;
use qrs_analysis::testsig;
use qrs_audio::rt::onset::OnsetDetector;
use qrs_formats::package::Package;

const RATE: u32 = 48000;

/// A 16-bit mono wav: its samples and rate.
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

/// What a recording of the part scores (its own chart, the same clock): the guitar's detector and the bass one.
fn recording(a: &qrs_formats::model::Arrangement, x: &[f32], rate: u32) {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // as a player's input is set: peaks about -6 dBFS
    let x: Vec<f32> = x.iter().map(|v| v * 0.5 / peak.max(1e-6)).collect();
    for bass in [false, true] {
        let mut det = OnsetDetector::new(rate);
        det.bass = bass;
        det.min_energy = 0.004;
        let mut found = Vec::new();
        for (k, c) in x.chunks(128).enumerate() {
            det.feed(c, (k * 128) as u64, |i, _| found.push(i));
        }
        let played = detect::build_groups(a)
            .iter()
            .map(|g| (0.0, g.t))
            .collect::<Vec<_>>();
        run(
            a,
            &x,
            rate,
            &found,
            if bass {
                "recording, bass detector"
            } else {
                "recording, old detector"
            },
            &played,
        );
    }
    // both: the old one's plucks, and the bass one's where the old found none close by
    let list = |bass: bool| {
        let mut det = OnsetDetector::new(rate);
        det.bass = bass;
        det.min_energy = 0.004;
        let mut found = Vec::new();
        for (k, c) in x.chunks(128).enumerate() {
            det.feed(c, (k * 128) as u64, |i, _| found.push(i));
        }
        found
    };
    let old = list(false);
    let near = (0.045 * rate as f64) as u64;
    let mut both = old.clone();
    both.extend(
        list(true)
            .into_iter()
            .filter(|b| !old.iter().any(|o| o.abs_diff(*b) < near)),
    );
    both.sort();
    let played = detect::build_groups(a)
        .iter()
        .map(|g| (0.0, g.t))
        .collect::<Vec<_>>();
    run(a, &x, rate, &both, "recording, both detectors", &played);
    // every sample a pluck candidate is too much: exact chart times instead, to part the note test from the detector
    let groups = detect::build_groups(a);
    let oracle: Vec<u64> = groups.iter().map(|g| (g.t * rate as f64) as u64).collect();
    let played: Vec<(f64, f64)> = groups.iter().map(|g| (0.0, g.t)).collect();
    run(
        a,
        &x,
        rate,
        &oracle,
        "recording, plucks at the chart's times",
        &played,
    );
}

struct Rec<'a> {
    x: &'a [f32],
    now: usize,
    rate: u32,
}

impl SampleSource for Rec<'_> {
    fn rate(&self) -> u32 {
        self.rate
    }
    fn written(&self) -> u64 {
        self.now as u64
    }
    fn read(&self, start: u64, out: &mut [f32]) -> bool {
        let s = start as usize;
        if s + out.len() > self.now {
            return false;
        }
        out.copy_from_slice(&self.x[s..s + out.len()]);
        true
    }
    fn latest(&self, out: &mut [f32]) -> bool {
        if out.len() > self.now {
            return false;
        }
        out.copy_from_slice(&self.x[self.now - out.len()..self.now]);
        true
    }
}

fn run(
    a: &qrs_formats::model::Arrangement,
    x: &[f32],
    rate: u32,
    plucks: &[u64],
    label: &str,
    played: &[(f64, f64)],
) {
    let mut s = Scorer::new(a);
    s.set_instrument(Instrument::Bass);
    let step = (0.01 * rate as f64) as usize;
    let (mut fed, mut now) = (0, 0);
    while now < x.len() {
        now = (now + step).min(x.len());
        let new: Vec<Pluck> = plucks
            .iter()
            .skip(fed)
            .take_while(|&&i| (i as usize) + 256 <= now)
            .map(|&i| Pluck {
                index: i,
                song_time: i as f64 / rate as f64,
            })
            .collect();
        fed += new.len();
        s.feed(new);
        let rec = Rec { x, now, rate };
        let live = Live {
            source: &rec,
            level: 0.05,
            latency: 0.0,
        };
        let t = now as f64 / rate as f64;
        s.update(t, Some(&live), t);
    }
    let c = s.counts();
    println!(
        "{label}: {:.1}%  hit {} late {} early {} miss {} (plucks {} for {} notes)",
        s.accuracy() * 100.0,
        c.hit,
        c.late,
        c.early,
        c.miss,
        plucks.len(),
        played.len()
    );
    let mut dts: Vec<f64> = s
        .groups
        .iter()
        .filter(|g| g.state == Some(Judgement::Hit))
        .map(|g| g.dt * 1000.0)
        .collect();
    dts.sort_by(f64::total_cmp);
    if !dts.is_empty() {
        println!(
            "  hit dt ms: median {:.1}, 5% {:.1}, 95% {:.1}",
            dts[dts.len() / 2],
            dts[dts.len() / 20],
            dts[dts.len() * 19 / 20]
        );
    }
    // by the gap to the next note
    let mut b = [[0u32; 4]; 5];
    let names = ["<90ms", "90-130", "130-200", "200-400", ">400"];
    for i in 0..s.groups.len() {
        let gap = s.groups.get(i + 1).map(|n| n.t - s.groups[i].t).unwrap_or(9.0);
        let k = if gap < 0.09 {
            0
        } else if gap < 0.13 {
            1
        } else if gap < 0.2 {
            2
        } else if gap < 0.4 {
            3
        } else {
            4
        };
        let j = match s.groups[i].state {
            Some(Judgement::Hit) => 0,
            Some(Judgement::Early) => 1,
            Some(Judgement::Late) => 2,
            _ => 3,
        };
        b[k][j] += 1;
    }
    for (n, r) in names.iter().zip(b) {
        println!(
            "  next note {n:8} hit {:5} early {:4} late {:4} miss {:4}",
            r[0], r[1], r[2], r[3]
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let jitter: f64 = args
        .get(2)
        .filter(|s| *s != "--wav")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0)
        / 1000.0;
    let keys = Default::default();
    let a = Package::open(std::path::Path::new(&args[1]), &keys, None)
        .unwrap()
        .load(Some("bass"), 1.0)
        .unwrap();
    if args.get(2).is_some_and(|s| s == "--wav") {
        let (x, rate) = read_wav(&args[3]);
        println!(
            "{} {} {}: {:.0} s at {rate}",
            a.artist,
            a.title,
            a.name,
            x.len() as f64 / rate as f64
        );
        recording(&a, &x, rate);
        return;
    }
    let groups = detect::build_groups(&a);
    let end = groups.last().map(|g| g.t).unwrap_or(0.0) + 2.0;
    println!(
        "{} {} {}: {} groups, tuning {:?}",
        a.artist,
        a.title,
        a.name,
        groups.len(),
        a.tuning
    );
    let mut noise = testsig::Noise::new(7);
    let played: Vec<(f64, f64)> = groups
        .iter()
        .map(|g| {
            (
                g.midis.iter().copied().min().unwrap_or(40) as f64,
                g.t + jitter * noise.sample() as f64,
            )
        })
        .collect();
    for (name, ks) in [("string", true), ("strong h2", false)] {
        let mut x = vec![0.0f32; (end * RATE as f64) as usize];
        for (k, &(m, at)) in played.iter().enumerate() {
            let note = if ks {
                testsig::ks_pluck(m, RATE, 1.2, 0.2, 0.75, k as u32)
            } else {
                testsig::additive_pluck(m, RATE, 1.2, 0.2, &[0.4, 1.0, 0.5, 0.3, 0.1], 0.01, 1.5)
            };
            let a0 = (at.max(0.0) * RATE as f64) as usize;
            for (i, v) in x.iter_mut().skip(a0).enumerate() {
                *v *= 1.0 - (i as f32 / 240.0).min(1.0);
            }
            testsig::mix_at(&mut x, a0, &note);
        }
        let hiss = testsig::hiss(x.len(), 0.0002, 4);
        testsig::mix_at(&mut x, 0, &hiss);
        let oracle: Vec<u64> = played
            .iter()
            .map(|p| (p.1.max(0.0) * RATE as f64) as u64)
            .collect();
        run(&a, &x, RATE, &oracle, &format!("{name}, exact plucks"), &played);
        let mut det = OnsetDetector::new(RATE);
        det.bass = true;
        let mut found = Vec::new();
        for (k, c) in x.chunks(128).enumerate() {
            det.feed(c, (k * 128) as u64, |i, _| found.push(i));
        }
        run(&a, &x, RATE, &found, &format!("{name}, live detector"), &played);
    }
}
