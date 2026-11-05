//! `quickrs-cli nam`: NAM captures and IRs off the board (new in quickRS, P31, D80): what a capture says about
//! itself, how much of a core it takes at a device rate, a WAV of a riff through it, and a standard-size test
//! capture (random weights, the A1 "standard" WaveNet's shape) for timing without a real one.

use std::path::PathBuf;
use std::time::Instant;

use qrs_audio::board::{Block, MAX_KNOBS, make_pedal, nam};

#[derive(clap::Subcommand)]
pub enum Args {
    /// What a .nam file says about itself (rate, gear, loudness, calibration, whether it has a cab)
    Info { file: PathBuf },
    /// Run a capture for 10 s of signal at a device rate and say how much of one core it needs
    Bench {
        file: PathBuf,
        #[arg(long, default_value_t = 48000)]
        rate: u32,
        /// samples per call, as the audio engine's buffer
        #[arg(long, default_value_t = 128)]
        buffer: usize,
    },
    /// How loud a riff comes out of a capture, beside our own amps with a cab (dBFS, RMS)
    Level {
        file: PathBuf,
        /// a pedal capture in front of it
        #[arg(long)]
        pedal: Option<PathBuf>,
        #[arg(long, default_value_t = 48000)]
        rate: u32,
    },
    /// A test capture with the standard WaveNet's shape and random weights (for `bench`; it sounds like nothing)
    Standard { out: PathBuf },
    /// A WAV of a plucked test riff through a capture (and an IR after it, if given)
    Render {
        file: PathBuf,
        #[arg(long)]
        ir: Option<PathBuf>,
        #[arg(short, default_value = "nam.wav")]
        o: PathBuf,
        #[arg(long, default_value_t = 48000)]
        rate: u32,
    },
}

pub fn run(args: Args) -> anyhow::Result<()> {
    match args {
        Args::Info { file } => {
            let json = std::fs::read_to_string(&file)?;
            let i = nam::inspect(&json).map_err(anyhow::Error::msg)?;
            println!("{}", file.display());
            println!("  name        {}", i.name.as_deref().unwrap_or("-"));
            println!("  gear        {}", i.gear.as_deref().unwrap_or("-"));
            println!("  modeled by  {}", i.modeled_by.as_deref().unwrap_or("-"));
            println!("  network     {} at {} Hz", i.architecture, i.rate);
            println!(
                "  levels      loudness {}, input {} dBu",
                i.loudness.map_or("-".into(), |l| format!("{l:.1} dB")),
                i.input_dbu.map_or("-".into(), |l| format!("{l:.1}"))
            );
            println!(
                "  cab         {}",
                match i.includes_cab {
                    Some(true) => "in the capture",
                    Some(false) => "not in it (use an IR cab after it)",
                    None => "not said",
                }
            );
        }
        Args::Bench { file, rate, buffer } => {
            let blk = load_amp(&file, 1)?;
            let mut p = make_pedal(&blk, rate);
            let n = rate as usize * 10;
            let mut x: Vec<f32> = (0..n).map(|i| 0.2 * (i as f32 * 0.031).sin()).collect();
            let t = Instant::now();
            for c in x.chunks_mut(buffer) {
                p.process(c);
            }
            let secs = t.elapsed().as_secs_f64();
            println!(
                "{:.0} ms for 10 s of audio at {rate} Hz in {buffer}-sample buffers: {:.1}% of one core",
                secs * 1000.0,
                secs / 10.0 * 100.0
            );
            assert!(x.iter().all(|v| v.is_finite()));
        }
        Args::Level { file, pedal, rate } => {
            let rms = |chain: &mut Vec<Box<dyn qrs_audio::board::Pedal>>| {
                let mut x = riff(rate);
                for c in x.chunks_mut(256) {
                    for p in chain.iter_mut() {
                        p.process(c);
                    }
                }
                let e = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
                10.0 * e.max(1e-12).log10()
            };
            let amp = load_amp(&file, 1)?;
            let mut chain = Vec::new();
            if let Some(pf) = pedal {
                nam::register_nam(3, &std::fs::read_to_string(pf)?).map_err(anyhow::Error::msg)?;
                let mut pb = Block::of("pedal_nam").expect("pedal_nam");
                pb.asset = 3;
                pb.v = [0.0; MAX_KNOBS];
                chain.push(make_pedal(&pb, rate));
            }
            chain.push(make_pedal(&amp, rate));
            println!("{:>24}  {:6.1} dB", "the capture", rms(&mut chain));
            for id in ["amp_clean", "amp_tweed", "amp_crunch", "amp_lead", "amp_modern"] {
                if let (Some(a), Some(c)) = (Block::of(id), Block::of("cab_4x12")) {
                    let v = rms(&mut vec![make_pedal(&a, rate), make_pedal(&c, rate)]);
                    println!("{:>24}  {:6.1} dB", format!("{id} + cab_4x12"), v);
                }
            }
        }
        Args::Standard { out } => {
            std::fs::write(&out, standard_capture(7))?;
            println!("wrote {}", out.display());
        }
        Args::Render { file, ir, o, rate } => {
            let amp = load_amp(&file, 1)?;
            let mut chain = vec![make_pedal(&amp, rate)];
            if let Some(ir) = ir {
                nam::register_ir(2, &std::fs::read(&ir)?).map_err(anyhow::Error::msg)?;
                let mut cab = Block::of("cab_ir").expect("cab_ir");
                cab.asset = 2;
                chain.push(make_pedal(&cab, rate));
            }
            let mut x = riff(rate);
            for c in x.chunks_mut(256) {
                for p in &mut chain {
                    p.process(c);
                }
            }
            let peak = x.iter().fold(0.0f32, |a, v| a.max(v.abs())).max(1e-6);
            let k = if peak > 0.9 { 0.9 / peak } else { 1.0 };
            let pcm: Vec<i16> = x
                .iter()
                .map(|v| ((v * k).clamp(-1.0, 1.0) * 32767.0) as i16)
                .collect();
            qrs_audio::decode::write_wav_i16(&o, rate, 1, &pcm)?;
            println!(
                "wrote {} (peak {:.1} dBFS before scaling)",
                o.display(),
                20.0 * peak.log10()
            );
        }
    }
    Ok(())
}

fn load_amp(file: &PathBuf, id: u32) -> anyhow::Result<Block> {
    let json = std::fs::read_to_string(file)?;
    nam::register_nam(id, &json).map_err(anyhow::Error::msg)?;
    let mut b = Block::of("amp_nam").expect("amp_nam");
    b.asset = id;
    b.v = [0.0; MAX_KNOBS];
    Ok(b)
}

/// Plucked strings (decaying sawtooth-ish tones), a bar of an E-minor riff, at about a guitar's level after our
/// input trim (peaks near -12 dBFS).
fn riff(rate: u32) -> Vec<f32> {
    let notes = [40, 47, 52, 55, 52, 47, 43, 45];
    let step = rate as usize / 4;
    let mut out = vec![0.0f32; step * notes.len() + rate as usize];
    for (k, &m) in notes.iter().enumerate() {
        let f = 440.0 * 2f32.powf((m as f32 - 69.0) / 12.0);
        for i in 0..rate as usize {
            let t = i as f32 / rate as f32;
            let env = (-t * 3.0).exp() * 0.25;
            let ph = (t * f).fract();
            out[k * step + i] += env * (ph * 2.0 - 1.0 + 0.5 * (2.0 * std::f32::consts::PI * f * t).sin());
        }
    }
    out
}

/// The A1 "standard" WaveNet (two arrays of ten layers, 16 then 8 channels, kernel 3) with random weights.
fn standard_capture(seed: u64) -> String {
    let dil: Vec<usize> = (0..10).map(|i| 1usize << i).collect();
    // weights: per array, rechannel + per layer (dilated conv + bias, input mixin, 1x1 + bias) + head (+ bias)
    let arr = |input: usize, ch: usize, head: usize, head_bias: bool| {
        input * ch
            + 10 * (ch * ch * 3 + ch + ch + ch * ch + ch)
            + head * ch
            + if head_bias { head } else { 0 }
    };
    let n = arr(1, 16, 8, false) + arr(16, 8, 1, true) + 1;
    let mut s = seed;
    let w: Vec<String> = (0..n)
        .map(|_| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            format!("{:.4}", ((s >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 0.3)
        })
        .collect();
    let d = format!("{dil:?}");
    format!(
        r#"{{"version": "0.5.4", "architecture": "WaveNet", "sample_rate": 48000,
"metadata": {{"name": "standard test (random)", "gear_type": "amp", "loudness": -18.0}},
"config": {{"layers": [
 {{"input_size": 1, "condition_size": 1, "channels": 16, "head_size": 8, "kernel_size": 3, "dilations": {d}, "activation": "Tanh", "gated": false, "head_bias": false}},
 {{"input_size": 16, "condition_size": 1, "channels": 8, "head_size": 1, "kernel_size": 3, "dilations": {d}, "activation": "Tanh", "gated": false, "head_bias": true}}
], "head": null, "head_scale": 0.02}},
"weights": [{}]}}"#,
        w.join(",")
    )
}
