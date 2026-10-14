//! `devices`, `play` and `synth`: the audio engine from the command line.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use qrs_audio::backend::Backend;
use qrs_audio::decode::{decode, to_song_buffer};
use qrs_audio::devices::{self, rank::rank_inputs};
use qrs_audio::{Audio, AudioPrefs, Choice, DevicePref};
use qrs_formats::{Keys, Package, song_audio::song_audio};

/// Library folders from a config.json (for finding the Rocksmith install).
pub fn library_dirs(config: Option<&Path>) -> Vec<PathBuf> {
    let p = config
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("config.json"));
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["library_dirs"].as_array().cloned())
        .map(|a| a.iter().filter_map(|d| d.as_str().map(PathBuf::from)).collect())
        .unwrap_or_default()
}

#[derive(clap::Args)]
pub struct DeviceArgs {
    /// config.json with library_dirs (to find the Rocksmith install and its RS_ASIO setup)
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Every string property of every WASAPI endpoint
    #[arg(long)]
    pub raw: bool,
    /// Run the planned route this many seconds, silently, and report the streams
    #[arg(long)]
    pub test: Option<u64>,
}

pub fn devices(args: DeviceArgs) -> anyhow::Result<()> {
    if args.raw {
        #[cfg(windows)]
        for (id, props) in qrs_audio::backend::wasapi::dump_properties() {
            println!("== {id}");
            for (k, v) in props {
                println!("   {k} = {v}");
            }
        }
        return Ok(());
    }
    let dirs = library_dirs(args.config.as_deref());
    let list = qrs_audio::backend::enumerate();
    let rs = devices::rocksmith::find_setup(&dirs);
    println!("Inputs (best first):");
    for r in rank_inputs(&list, rs.as_ref()) {
        let d = &r.device;
        println!(
            "  {:>5}  {:<52} {:>2} ch {:>6} Hz  {:?}: {}{}",
            r.score,
            d.label(),
            d.input_channels,
            d.default_rate,
            r.kind,
            r.reason,
            d.hardware
                .usb
                .map(|(v, p)| format!("  [USB {v:04X}:{p:04X}]"))
                .unwrap_or_default()
        );
    }
    println!("\nOutputs:");
    for d in list
        .iter()
        .filter(|d| d.output_channels > 0 && d.id.backend != Backend::Null)
    {
        let def = if d.is_default_output { "  [default]" } else { "" };
        println!(
            "  {:<52} {:>2} ch {:>6} Hz{def}",
            d.label(),
            d.output_channels,
            d.default_rate
        );
    }
    match &rs {
        Some(s) => {
            println!(
                "\nRocksmith 2014 at {}: exclusive {}, latency buffer {}, microphone {}",
                s.folder.display(),
                s.exclusive_mode,
                s.latency_buffer,
                s.microphone
            );
            if let Some(a) = &s.rs_asio {
                let out = if a.output_driver.is_empty() {
                    "WASAPI".to_string()
                } else {
                    a.output_driver.clone()
                };
                println!(
                    "  RS_ASIO: ASIO {} | inputs {:?} | output {out} | buffer {} {:?}",
                    a.enable_asio, a.inputs, a.buffer_mode, a.custom_buffer
                );
            }
        }
        None => println!("\nRocksmith 2014: not found (give --config with your library folders)"),
    }
    let plan = devices::plan(&AudioPrefs::default(), &list, rs.as_ref());
    println!("\nAutomatic choice:");
    let guitar = plan
        .input
        .as_ref()
        .map(|i| format!("{} ({})", i.device.label(), i.reason))
        .unwrap_or_else(|| "none".into());
    let channel = plan
        .route
        .input
        .as_ref()
        .map(|i| {
            if i.channel < 0 {
                "found from the signal".to_string()
            } else {
                format!("{}", i.channel + 1)
            }
        })
        .unwrap_or_default();
    println!("  guitar: {guitar}, channel {channel}");
    println!(
        "  sound:  {}",
        plan.output
            .as_ref()
            .map(|o| o.label())
            .unwrap_or_else(|| "none (silent)".into())
    );
    println!(
        "  buffer: {:?}, exclusive: {}",
        plan.route.buffer, plan.route.exclusive
    );
    for n in &plan.notes {
        println!("  note: {n}");
    }
    if let Some(secs) = args.test {
        let audio = Audio::start(AudioPrefs::default(), dirs);
        audio.engine.set_monitor(false);
        audio.engine.set_song_gain(0.0);
        audio.engine.settle(Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(300));
        audio.engine.reset_stats();
        std::thread::sleep(Duration::from_secs(secs));
        let st = audio.engine.status();
        println!("\nTest ({secs} s, silent):");
        for (what, i) in [("input", &st.input), ("output", &st.output)] {
            if let Some(i) = i {
                println!("  {what}: {} ({}), block {}", i.device_name, i.mode, i.block);
            }
        }
        if let Some(e) = &st.input_error {
            println!("  input error: {e}");
        }
        if let Some(e) = &st.output_error {
            println!("  output error: {e}");
        }
        let s = audio.engine.stats();
        println!("  {s:?}, worst callback {:.3} ms", s.max_work_ns as f64 / 1e6);
    }
    Ok(())
}

#[derive(clap::Args)]
pub struct PlayArgs {
    pub file: PathBuf,
    #[arg(long)]
    pub song_key: Option<String>,
    /// Start here (seconds)
    #[arg(long, default_value_t = 0.0)]
    pub from: f64,
    /// Stop after this many seconds
    #[arg(long)]
    pub seconds: Option<f64>,
    /// Song volume 0..1
    #[arg(long, default_value_t = 0.8)]
    pub volume: f32,
    #[arg(long, default_value_t = 0.8)]
    pub guitar_volume: f32,
    #[arg(long)]
    pub no_monitor: bool,
    /// Part of the input device's name ("none" for no guitar; default: automatic)
    #[arg(long)]
    pub input: Option<String>,
    /// Part of the output device's name ("none" for silent)
    #[arg(long)]
    pub output: Option<String>,
    #[arg(long)]
    pub exclusive: bool,
    #[arg(long)]
    pub buffer: Option<u32>,
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Play slower (0.25..1) or faster (up to 1.5), keeping the pitch
    #[arg(long, default_value_t = 1.0)]
    pub speed: f64,
}

fn choice(part: Option<&str>, input: bool) -> Choice {
    let Some(part) = part else { return Choice::Auto };
    if part.eq_ignore_ascii_case("none") {
        return Choice::Off;
    }
    let p = part.to_lowercase();
    qrs_audio::backend::enumerate()
        .into_iter()
        .find(|d| {
            d.label().to_lowercase().contains(&p)
                && if input {
                    d.input_channels > 0
                } else {
                    d.output_channels > 0
                }
        })
        .map(|d| Choice::Device(DevicePref::of(&d)))
        .unwrap_or(Choice::Auto)
}

pub fn play(keys: &Keys, a: PlayArgs) -> anyhow::Result<()> {
    let t0 = Instant::now();
    let mut pkg = Package::open(&a.file, keys, a.song_key.as_deref())?;
    let enc = song_audio(&mut pkg, false)?.ok_or_else(|| anyhow::anyhow!("no audio"))?;
    let pcm = decode(&enc)?;
    let prefs = AudioPrefs {
        input: choice(a.input.as_deref(), true),
        output: choice(a.output.as_deref(), false),
        exclusive: a.exclusive,
        buffer: a.buffer,
        ..Default::default()
    };
    let audio = Audio::start(prefs, library_dirs(a.config.as_deref()));
    let e = &audio.engine;
    e.settle(Duration::from_secs(3));
    e.set_song_gain(a.volume);
    e.set_guitar_gain(a.guitar_volume);
    e.set_monitor(!a.no_monitor);
    let song = to_song_buffer(pcm, e.rate())?;
    let shared = std::sync::Arc::new(song.clone());
    e.load_song(song);
    let _job = if (a.speed - 1.0).abs() > 1e-6 {
        let t = Instant::now();
        let job = qrs_audio::stretch::StretchJob::start(shared, a.speed, a.from);
        e.set_speed(a.speed, Some(job.buffer.clone()));
        let from_frame = (a.from / a.speed * e.rate() as f64) as usize;
        while !job.buffer.is_ready(from_frame, from_frame + 4800) && t.elapsed().as_secs() < 30 {
            std::thread::sleep(Duration::from_millis(5));
        }
        println!(
            "  speed {:.0}%: first audio ready in {:.0} ms",
            a.speed * 100.0,
            t.elapsed().as_secs_f64() * 1000.0
        );
        Some((job, t))
    } else {
        None
    };
    let st = e.status();
    println!(
        "{} - {} ({:.0} s), ready in {:.2} s",
        pkg.artist(),
        pkg.title(),
        e.duration(),
        t0.elapsed().as_secs_f64()
    );
    let inp = st
        .input
        .as_ref()
        .map(|i| format!("{} [{}]", i.device_name, i.mode))
        .unwrap_or_else(|| st.input_error.clone().unwrap_or_else(|| "none".into()));
    let out = st
        .output
        .as_ref()
        .map(|i| format!("{} [{}]", i.device_name, i.mode))
        .unwrap_or_default();
    println!("  in: {inp}\n  out: {out}");
    std::thread::sleep(Duration::from_millis(50));
    e.play(Some(a.from));
    let end = a.seconds.map(|s| Instant::now() + Duration::from_secs_f64(s));
    let mut plucks = 0usize;
    while e.playing() && end.is_none_or(|t| Instant::now() < t) {
        std::thread::sleep(Duration::from_millis(250));
        plucks += e.take_onsets().len();
        let m = e.meters();
        print!(
            "\r  {:6.1} s  guitar {:>4.0}%  plucks {plucks}   ",
            e.position(),
            m.peak * 100.0
        );
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        for ev in audio.events() {
            println!("\n  {ev:?}");
        }
    }
    e.pause();
    let stopped_at = e.position();
    if let Some((job, t)) = &_job {
        while !job.done() && t.elapsed().as_secs() < 120 {
            std::thread::sleep(Duration::from_millis(20));
        }
        println!("\n  whole song stretched in {:.2} s", t.elapsed().as_secs_f64());
    }
    let s = e.stats();
    println!("\n  stopped at {stopped_at:.1} s; {s:?}");
    Ok(())
}

#[derive(clap::Args)]
pub struct SynthArgs {
    /// .psarc, .feedpak, .sng or .xml
    pub file: PathBuf,
    /// part: lead, rhythm, bass, combo, a number, or a prefix
    #[arg(long)]
    pub arr: Option<String>,
    /// difficulty between 0 and 1 (1 = the full chart)
    #[arg(long, default_value_t = 1.0)]
    pub mastery: f64,
    /// Chart seconds to start at
    #[arg(long, default_value_t = 0.0)]
    pub from: f64,
    /// Chart seconds to stop at (at most a minute after --from)
    #[arg(long, default_value_t = 10.0)]
    pub to: f64,
    /// Slower (0.25..1), keeping the pitch
    #[arg(long, default_value_t = 1.0)]
    pub speed: f64,
    /// clean, crunch, lead, bass, acoustic, or dry
    #[arg(long, default_value = "clean")]
    pub tone: String,
    /// Loop the passage (played 3 times; with --out, one loop is written, to be played round)
    #[arg(long = "loop")]
    pub looped: bool,
    /// Write a WAV here instead of playing it
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Part of the output device's name (default: automatic)
    #[arg(long)]
    pub output: Option<String>,
    #[arg(long)]
    pub config: Option<PathBuf>,
}

pub fn synth(arr: &qrs_formats::Arrangement, a: SynthArgs) -> anyhow::Result<()> {
    use qrs_audio::synth::{self, SynthPlayer, SynthState};
    use qrs_audio::tone::Preset;
    let tone = match a.tone.to_lowercase().as_str() {
        "dry" | "none" => None,
        t => Some(
            Preset::ALL
                .into_iter()
                .find(|p| p.label().eq_ignore_ascii_case(t))
                .ok_or_else(|| anyhow::anyhow!("unknown tone {t:?}"))?
                .params(),
        ),
    };
    let notes = synth::notes_of(arr);
    let opens = arr.open_string_midi();
    let speed = a.speed.clamp(0.25, 1.0);
    let count = notes.iter().filter(|n| a.from <= n.t && n.t < a.to).count();
    if count == 0 {
        anyhow::bail!("no notes between {} s and {} s", a.from, a.to);
    }
    println!(
        "{} - {} ({}): {count} notes from {:.2} s to {:.2} s at {:.0}%",
        arr.artist,
        arr.title,
        arr.name,
        a.from,
        a.to,
        speed * 100.0
    );
    if let Some(path) = &a.out {
        let t = Instant::now();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let place = |t: f64| t;
        let o = synth::RenderOptions {
            rate: 48000,
            speed,
            bass: arr.is_bass,
            tone,
            looped: a.looped,
            seed: 1,
            place: &place,
            cancel: &cancel,
        };
        let clip = synth::render(&notes, &opens, a.from, a.to, &o).unwrap_or_default();
        let pcm: Vec<i16> = clip.iter().map(|&v| qrs_audio::decode::f32_to_i16(v)).collect();
        qrs_audio::decode::write_wav_i16(path, 48000, 2, &pcm)?;
        println!(
            "  rendered {:.1} s in {:.0} ms -> {}",
            clip.len() as f64 / 96000.0,
            t.elapsed().as_secs_f64() * 1000.0,
            path.display()
        );
        return Ok(());
    }
    let prefs = AudioPrefs {
        input: Choice::Off,
        output: choice(a.output.as_deref(), false),
        ..Default::default()
    };
    let audio = Audio::start(prefs, library_dirs(a.config.as_deref()));
    let e = &audio.engine;
    e.settle(Duration::from_secs(3));
    let mut p = SynthPlayer::default();
    p.start(
        notes,
        opens,
        a.from,
        a.to,
        speed,
        arr.is_bass,
        a.looped,
        tone.unwrap_or_else(|| Preset::Clean.params()),
        e.rate(),
        std::sync::Arc::new(|t| t),
    );
    let t = Instant::now();
    while p.state == SynthState::Rendering && t.elapsed().as_secs() < 30 {
        p.update(e);
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("  rendered in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let length = (p.b - p.a) / speed;
    let until = Instant::now() + Duration::from_secs_f64(if a.looped { 3.0 * length } else { length + 1.5 });
    while p.state == SynthState::Playing && Instant::now() < until {
        p.update(e);
        if let Some(t) = p.position(e) {
            print!("\r  {t:6.2} s ");
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    p.stop(e);
    std::thread::sleep(Duration::from_millis(300));
    println!();
    Ok(())
}
