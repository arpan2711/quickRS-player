//! `quickrs-cli board`: the pedalboard's models, and a listening set (PLAN.md P14.5): synth riffs played dry
//! through every model (each in a chain that shows it off) and every starting board, and a song's part through
//! its own tones (changing where its chart does), written as WAVs, with a spectral summary of each. `live` plays
//! your guitar through a starting board on the real interface, reporting levels, dropouts and the time each
//! callback took, and can record the dry guitar and render it through every starting board.

use std::path::{Path, PathBuf};

use qrs_audio::board::{self, Block, Board, Kind, MODELS, catalog};
use qrs_audio::synth::{self, SynthNote};
use qrs_formats::Techs;

#[derive(clap::Args)]
pub struct Args {
    /// list (the models), render (the listening set), listen (P37.2: boards and chains measured), live, or rigs (how a
    /// library's song tones are matched)
    #[arg(default_value = "list")]
    pub action: String,
    /// Folder for the WAVs (render)
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// A song (.psarc or .feedpak) to play through its own tones
    #[arg(long)]
    pub song: Option<PathBuf>,
    /// The song's part: lead, rhythm, bass...
    #[arg(long)]
    pub arr: Option<String>,
    /// Chart seconds to start at (with --song)
    #[arg(long, default_value_t = 0.0)]
    pub from: f64,
    /// Chart seconds to stop at (with --song)
    #[arg(long, default_value_t = 30.0)]
    pub to: f64,
    /// Only the models whose id contains this
    #[arg(long)]
    pub only: Option<String>,
    /// live: the starting board to play, by name (default Clean)
    #[arg(long)]
    pub preset: Option<String>,
    /// live: seconds to run
    #[arg(long, default_value_t = 30.0)]
    pub seconds: f64,
    /// live: step to the next starting board every this many seconds (whole-board swaps)
    #[arg(long)]
    pub cycle: Option<f64>,
    /// live: sweep the amp's gain up and down, as dragging its knob does (in-place edits)
    #[arg(long)]
    pub sweep: bool,
    /// live: part of the input device's name (default: automatic)
    #[arg(long)]
    pub input: Option<String>,
    /// live: part of the output device's name ("none" for silent; default: automatic)
    #[arg(long)]
    pub output: Option<String>,
    /// live: buffer in frames (ASIO, exclusive WASAPI)
    #[arg(long)]
    pub buffer: Option<u32>,
    /// live: the guitar's channel, 0-based (default: found from the signal)
    #[arg(long)]
    pub channel: Option<i32>,
    /// live: how loud the guitar is monitored, 0..1
    #[arg(long, default_value_t = 0.6)]
    pub guitar_volume: f32,
    /// live: write the dry guitar to this WAV, and it through every starting board beside it
    #[arg(long)]
    pub record: Option<PathBuf>,
    /// live: wait up to this many seconds for the guitar (a note over -40 dBFS) before starting the clock
    #[arg(long)]
    pub wait: Option<f64>,
    /// render: a recording of your guitar (a WAV from `live --record`) to play through every starting board
    #[arg(long)]
    pub guitar: Option<PathBuf>,
    /// rigs: song folders to scan (default: the config's library folders and their feedpak folders); listen: the NAM
    /// captures folder (default: the app's)
    #[arg(long = "dir")]
    pub dirs: Vec<PathBuf>,
    /// rigs: config.json with library_dirs (default: the app's own)
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// rigs: worker threads
    #[arg(long, default_value_t = 8)]
    pub jobs: usize,
}

const RATE: u32 = 48000;

fn note(t: f64, string: i32, fret: i32, sus: f64, tech: Techs, in_chord: bool) -> SynthNote {
    SynthNote {
        t,
        string,
        fret,
        sus,
        tech,
        slide_to: -1,
        bend: 0.0,
        in_chord,
    }
}

/// Power-chord chugs at 120 bpm: E5 with palm-muted eighths, then A5 and G5 let ring.
pub(crate) fn chug() -> Vec<SynthNote> {
    let mut n = Vec::new();
    let e = 0.25;
    let mut t = 0.0;
    for bar in 0..2 {
        for i in 0..8 {
            let pm = if i % 4 == 3 { Techs::NONE } else { Techs::PALM_MUTE };
            for (s, f) in [(0, 0), (1, 2)] {
                n.push(note(t, s, f, 0.2, pm, true));
            }
            t += e;
        }
        let (root, fret) = if bar == 0 { (1, 0) } else { (0, 3) };
        for (s, f) in [(root, fret), (root + 1, fret + 2), (root + 2, fret + 2)] {
            n.push(note(t, s, f, 0.9, Techs::NONE, true));
        }
        t += 4.0 * e;
    }
    n
}

/// A minor-pentatonic lick with a bend and vibrato at the end.
pub(crate) fn lick() -> Vec<SynthNote> {
    let seq: [(f64, i32, i32, f64); 10] = [
        (0.0, 3, 7, 0.0),
        (0.25, 3, 9, 0.0),
        (0.5, 4, 8, 0.0),
        (0.75, 4, 10, 0.0),
        (1.0, 5, 8, 0.0),
        (1.25, 5, 10, 0.0),
        (1.5, 4, 10, 1.0),
        (2.25, 5, 8, 0.0),
        (2.5, 4, 10, 0.0),
        (2.75, 3, 9, 0.0),
    ];
    let mut out: Vec<SynthNote> = seq
        .iter()
        .map(|&(t, s, f, b)| SynthNote {
            bend: b,
            tech: if b > 0.0 { Techs::BEND } else { Techs::NONE },
            ..note(t, s, f, if b > 0.0 { 0.7 } else { 0.25 }, Techs::NONE, false)
        })
        .collect();
    out.push(note(3.0, 3, 7, 1.5, Techs::VIBRATO, false));
    out
}

/// Open chords strummed: E, A, D, G, a beat each, then E let ring.
pub(crate) fn chords() -> Vec<SynthNote> {
    let shapes: [&[(i32, i32)]; 5] = [
        &[(0, 0), (1, 2), (2, 2), (3, 1), (4, 0), (5, 0)],
        &[(1, 0), (2, 2), (3, 2), (4, 2), (5, 0)],
        &[(2, 0), (3, 2), (4, 3), (5, 2)],
        &[(0, 3), (1, 2), (2, 0), (3, 0), (4, 0), (5, 3)],
        &[(0, 0), (1, 2), (2, 2), (3, 1), (4, 0), (5, 0)],
    ];
    let mut n = Vec::new();
    for (i, shape) in shapes.iter().enumerate() {
        let t = i as f64 * 0.75;
        let ring = if i == 4 { 2.5 } else { 0.7 };
        for (k, &(s, f)) in shape.iter().enumerate() {
            n.push(note(t + k as f64 * 0.012, s, f, ring, Techs::NONE, true));
        }
    }
    n
}

/// The notes played dry (mono), peaks at -10 dBFS like a guitar into an interface.
pub(crate) fn dry(notes: &[SynthNote], bass: bool) -> Vec<f32> {
    let opens: Vec<i32> = if bass {
        vec![28, 33, 38, 43]
    } else {
        vec![40, 45, 50, 55, 59, 64]
    };
    let end = notes.iter().map(|n| n.t + n.sus).fold(0.0, f64::max) + 1.5;
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let place = |t: f64| t;
    let o = synth::RenderOptions {
        rate: RATE,
        speed: 1.0,
        bass,
        tone: None,
        looped: false,
        seed: 3,
        place: &place,
        cancel: &cancel,
    };
    let st = synth::render(notes, &opens, 0.0, end, &o).unwrap_or_default();
    let mono: Vec<f32> = st.chunks(2).map(|c| c[0]).collect();
    let peak = mono.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    mono.iter().map(|v| v * 0.316 / peak).collect()
}

pub(crate) fn write(path: &Path, x: &[f32]) -> anyhow::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let pcm: Vec<i16> = x
        .iter()
        .flat_map(|&v| {
            let s = qrs_audio::decode::f32_to_i16(v);
            [s, s]
        })
        .collect();
    qrs_audio::decode::write_wav_i16(path, RATE, 2, &pcm)?;
    Ok(())
}

/// Level, crest factor and where the energy sits (a 4096-point averaged spectrum).
fn summary(x: &[f32]) -> String {
    use realfft::RealFftPlanner;
    let n = 4096;
    let rms = (x.iter().map(|v| (v * v) as f64).sum::<f64>() / x.len().max(1) as f64).sqrt();
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);
    let mut acc = vec![0.0f64; n / 2 + 1];
    let mut frames = 0;
    let mut i = 0;
    while i + n <= x.len() {
        let mut buf: Vec<f64> = (0..n)
            .map(|k| x[i + k] as f64 * (0.5 - 0.5 * (std::f64::consts::TAU * k as f64 / n as f64).cos()))
            .collect();
        let mut spec = fft.make_output_vec();
        if fft.process(&mut buf, &mut spec).is_ok() {
            for (a, c) in acc.iter_mut().zip(&spec) {
                *a += c.norm_sqr();
            }
            frames += 1;
        }
        i += n / 2;
    }
    let bin = RATE as f64 / n as f64;
    let total: f64 = acc.iter().sum::<f64>().max(1e-30);
    let band = |lo: f64, hi: f64| {
        acc.iter()
            .enumerate()
            .filter(|(k, _)| *k as f64 * bin >= lo && (*k as f64 * bin) < hi)
            .map(|(_, v)| v)
            .sum::<f64>()
            / total
    };
    let centroid = acc
        .iter()
        .enumerate()
        .map(|(k, v)| k as f64 * bin * v)
        .sum::<f64>()
        / total;
    let _ = frames;
    format!(
        "{:+6.1} dBFS rms  crest {:4.1} dB  centroid {:5.0} Hz  <150 {:4.1}%  150-1k {:4.1}%  1-5k {:4.1}%  >5k {:4.2}%",
        20.0 * rms.max(1e-9).log10(),
        20.0 * (peak / rms.max(1e-9)).log10(),
        centroid,
        100.0 * band(0.0, 150.0),
        100.0 * band(150.0, 1000.0),
        100.0 * band(1000.0, 5000.0),
        100.0 * band(5000.0, 24000.0),
    )
}

fn b(id: &str) -> Block {
    Block::of(id).unwrap_or_else(|| panic!("no model {id}"))
}

/// A chain that shows a model off, and the riff it plays.
fn showcase(id: &str, kind: Kind) -> (Board, &'static str) {
    let m = b(id);
    match kind {
        Kind::Dynamics => (
            Board::new(vec![m, b("amp_clean").with("gain", 3.5), b("cab_2x12")]),
            "lick",
        ),
        Kind::Drive if id == "bass_drive" => (Board::new(vec![m, b("amp_bass"), b("cab_8x10")]), "bass"),
        Kind::Drive => (
            Board::new(vec![m, b("amp_crunch").with("gain", 3.0), b("cab_4x12")]),
            "chug",
        ),
        Kind::Fuzz => (
            Board::new(vec![m, b("amp_clean").with("gain", 5.0), b("cab_2x12")]),
            "lick",
        ),
        Kind::Amp => {
            let cab = match id {
                "amp_clean" | "amp_chime" => "cab_2x12",
                "amp_tweed" => "cab_1x12",
                "amp_bass" => "cab_8x10",
                _ => "cab_4x12",
            };
            let riff = match id {
                "amp_bass" => "bass",
                "amp_clean" | "amp_chime" | "amp_tweed" => "chords",
                _ => "chug",
            };
            (Board::new(vec![m, b(cab)]), riff)
        }
        Kind::Cab if id.starts_with("cab_8") || id.starts_with("cab_1x15") => {
            (Board::new(vec![b("amp_bass"), m]), "bass")
        }
        Kind::Cab => (Board::new(vec![b("amp_crunch").with("gain", 5.0), m]), "chug"),
        Kind::Eq => {
            let mut eq = m;
            // a smile: lows and highs up, mids down
            for (i, k) in eq.def().knobs.iter().enumerate() {
                if k.key.starts_with('b') && k.key != "bass" {
                    let n = eq
                        .def()
                        .knobs
                        .iter()
                        .filter(|k| k.key.starts_with('b') && k.key != "bass")
                        .count();
                    let x = i as f32 / (n.max(2) - 1) as f32;
                    eq.v[i] = 8.0 * (2.0 * x - 1.0).powi(2) - 4.0;
                }
            }
            if id == "studio_eq" {
                eq.set("bass", 4.0)
                    .set("lo_mid", -5.0)
                    .set("hi_mid", 3.0)
                    .set("treble", 4.0);
            }
            (
                Board::new(vec![b("amp_crunch").with("gain", 5.0), b("cab_4x12"), eq]),
                "chug",
            )
        }
        Kind::Filter => {
            let mut f = m;
            if id == "wah" {
                f.set("mode", 1.0).set("sens", 6.0);
            }
            (
                Board::new(vec![f, b("amp_clean").with("gain", 4.5), b("cab_2x12")]),
                "lick",
            )
        }
        Kind::Modulation => (
            Board::new(vec![b("amp_clean").with("gain", 3.5), b("cab_2x12"), m]),
            "chords",
        ),
        Kind::Delay | Kind::Reverb => (
            Board::new(vec![b("amp_clean").with("gain", 3.5), b("cab_2x12"), m]),
            "lick",
        ),
    }
}

pub fn run(keys: &qrs_formats::Keys, a: Args) -> anyhow::Result<()> {
    if a.action == "list" {
        for kind in Kind::ALL {
            println!("{}", kind.label().to_uppercase());
            for (_, m) in catalog::of_kind(kind) {
                let knobs: Vec<&str> = m.knobs.iter().map(|k| k.label).collect();
                println!(
                    "  {:<18} {:<34} after {}  [{}]",
                    m.id,
                    m.name,
                    m.after,
                    knobs.join(", ")
                );
            }
        }
        println!("{} models", MODELS.len());
        return Ok(());
    }
    if a.action == "live" {
        return live(&a);
    }
    if a.action == "rigs" {
        // --out: also write the report there
        let dirs = crate::library_cmd::folders(a.config.as_deref(), &a.dirs);
        return crate::board_rigs::run(keys, &dirs, a.jobs, a.out.as_deref(), a.only.as_deref());
    }
    if a.action == "loudness" {
        // P37.6c: the amps' loudness by gain, as `board/loudness.rs` holds it
        print!("{}", crate::listen_cmd::loudness_table());
        return Ok(());
    }
    if a.action == "idle" {
        // P38.2: what each board puts out while you aren't playing (silence, an interface's floor, a guitar's noise)
        return crate::idle_cmd::run(a.out.as_deref(), a.only.as_deref(), a.dirs.first().cloned());
    }
    if a.action == "listen" {
        // P37.2: every starting board and a matrix of chains on synthetic DI, measured (`listen_cmd`)
        let out = a.out.clone().unwrap_or_else(|| PathBuf::from("board-listen"));
        // (--dir: the NAM captures library to read, else the app's)
        return crate::listen_cmd::run(&out, a.only.as_deref(), a.dirs.first().cloned());
    }
    if a.action != "render" {
        anyhow::bail!(
            "unknown action {:?} (list, render, listen, idle, loudness, live or rigs)",
            a.action
        );
    }
    let out = a.out.clone().unwrap_or_else(|| PathBuf::from("board-renders"));
    let riffs: Vec<(&str, Vec<f32>)> = vec![
        ("chug", dry(&chug(), false)),
        ("lick", dry(&lick(), false)),
        ("chords", dry(&chords(), false)),
        ("bass", dry(&chug(), true)),
    ];
    let riff = |name: &str| &riffs.iter().find(|r| r.0 == name).expect("riff").1;
    if let Some(song) = &a.song {
        return render_song(keys, song, &a, &out);
    }
    if let Some(g) = &a.guitar {
        return render_guitar(g);
    }
    for (name, x) in &riffs {
        write(&out.join("dry").join(format!("{name}.wav")), x)?;
        println!("{:<26} {}", format!("dry/{name}"), summary(x));
    }
    for (name, board) in board::presets() {
        let x = riff(match name {
            "Bass" => "bass",
            "Clean" | "Ambient" | "Acoustic" | "Blues" => "chords",
            "Lead" | "Fuzz" => "lick",
            _ => "chug",
        });
        let y = board::render(&board, RATE, x);
        let path = out.join("boards").join(format!("{}.wav", name.to_lowercase()));
        write(&path, &y)?;
        println!("{:<26} {}", format!("boards/{name}"), summary(&y));
    }
    for (i, kind) in Kind::ALL.iter().enumerate() {
        for (_, m) in catalog::of_kind(*kind) {
            if a.only.as_deref().is_some_and(|o| !m.id.contains(o)) {
                continue;
            }
            let (board, r) = showcase(m.id, *kind);
            let y = board::render(&board, RATE, riff(r));
            let file = format!("{:02}-{}-{}.wav", i + 1, kind.label().to_lowercase(), m.id);
            write(&out.join("models").join(&file), &y)?;
            println!("{:<26} {}", m.id, summary(&y));
        }
    }
    println!("-> {}", out.display());
    Ok(())
}

/// A song's part through each of its own tones, and once through with the tone changes where the chart has them.
fn render_song(keys: &qrs_formats::Keys, song: &Path, a: &Args, out: &Path) -> anyhow::Result<()> {
    let arr = crate::load(keys, song, a.arr.as_deref(), 1.0)?;
    if arr.rig.is_empty() {
        anyhow::bail!("{}: no tone definitions in this song", song.display());
    }
    let notes = synth::notes_of(&arr);
    let opens = arr.open_string_midi();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let place = |t: f64| t;
    let o = synth::RenderOptions {
        rate: RATE,
        speed: 1.0,
        bass: arr.is_bass,
        tone: None,
        looped: false,
        seed: 1,
        place: &place,
        cancel: &cancel,
    };
    let st = synth::render(&notes, &opens, a.from, a.to, &o).unwrap_or_default();
    let mono: Vec<f32> = st.chunks(2).map(|c| c[0]).collect();
    let peak = mono.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    let x: Vec<f32> = mono.iter().map(|v| v * 0.316 / peak).collect();
    let dir = out.join(format!("{} - {} ({})", arr.artist, arr.title, arr.name));
    write(&dir.join("dry.wav"), &x)?;
    println!(
        "{} - {} ({}): {} tones",
        arr.artist,
        arr.title,
        arr.name,
        arr.rig.len()
    );
    let boards: Vec<(String, Board)> = arr
        .rig
        .iter()
        .map(|t| (t.key.clone(), board::rs_gear::board_for(t)))
        .collect();
    for (t, (key, bd)) in arr.rig.iter().zip(&boards) {
        let ids: Vec<&str> = bd.blocks.iter().map(|b| b.def().id).collect();
        let gear: Vec<&str> = t.gear.iter().map(|g| g.key.as_str()).collect();
        let y = board::render(bd, RATE, &x);
        write(&dir.join(format!("tone {}.wav", t.label())), &y)?;
        println!("  {key:<28} {}", summary(&y));
        println!("      {:?} -> {}", gear, ids.join(" > "));
    }
    // the part as you'd hear it: tone changes at the chart's times (30 ms crossfades, like the app)
    let at = |t: f64| -> usize {
        let base = boards.iter().position(|(k, _)| *k == arr.base_tone).unwrap_or(0);
        arr.tones
            .iter()
            .filter(|c| c.time <= t)
            .rev()
            .find_map(|c| boards.iter().position(|(k, _)| *k == c.name))
            .unwrap_or(base)
    };
    let mut renders: Vec<Vec<f32>> = boards.iter().map(|(_, bd)| board::render(bd, RATE, &x)).collect();
    let mut follow = vec![0.0f32; x.len()];
    let fade = (RATE as usize) * 30 / 1000;
    let mut cur = at(a.from);
    let mut since = fade;
    for (i, v) in follow.iter_mut().enumerate() {
        let t = a.from + i as f64 / RATE as f64;
        let want = at(t);
        if want != cur {
            cur = want;
            since = 0;
        }
        let w = (since as f32 / fade as f32).min(1.0);
        let prev = if since < fade { at(t - 0.031) } else { cur };
        *v = renders[prev][i] * (1.0 - w) + renders[cur][i] * w;
        since += 1;
    }
    renders.clear();
    write(&dir.join("following the chart.wav"), &follow)?;
    println!("  following the chart        {}", summary(&follow));
    println!("-> {}", dir.display());
    Ok(())
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-6).log10()
}

/// Your guitar through a starting board on the real interface: levels, dropouts, how long the callbacks took,
/// and (with --record) the dry guitar written and rendered through every starting board.
fn live(a: &Args) -> anyhow::Result<()> {
    use qrs_audio::tone::ToneSpec;
    use qrs_audio::{Audio, AudioPrefs};
    use std::io::Write as _;
    use std::time::{Duration, Instant};
    let presets = board::presets();
    let mut at = match &a.preset {
        Some(n) => presets
            .iter()
            .position(|(p, _)| p.eq_ignore_ascii_case(n))
            .ok_or_else(|| anyhow::anyhow!("no starting board called {n:?}"))?,
        None => 0,
    };
    let prefs = AudioPrefs {
        input: crate::audio_cmds::choice(a.input.as_deref(), true),
        output: crate::audio_cmds::choice(a.output.as_deref(), false),
        buffer: a.buffer,
        channel: a.channel,
        ..Default::default()
    };
    let audio = Audio::start(prefs, crate::audio_cmds::library_dirs(None));
    let e = &audio.engine;
    e.settle(Duration::from_secs(4));
    e.set_guitar_gain(a.guitar_volume);
    e.set_monitor(true);
    let mut board_now = presets[at].1.renewed();
    e.set_tone(ToneSpec::Board {
        board: board_now.clone(),
    });
    let st = e.status();
    let side = |s: &Option<qrs_audio::backend::StreamInfo>, err: &Option<String>| match s {
        Some(i) => format!(
            "{} [{}] {} Hz, {} frames ({:.1} ms) a callback, device latency {:.1} ms",
            i.device_name,
            i.mode,
            i.rate,
            i.block,
            i.block as f64 * 1000.0 / i.rate.max(1) as f64,
            i.latency * 1000.0
        ),
        None => format!("none ({})", err.clone().unwrap_or_default()),
    };
    println!("in:  {}", side(&st.input, &st.input_error));
    println!("out: {}", side(&st.output, &st.output_error));
    println!("one device both ways: {}", st.duplex);
    if let (Some(i), Some(o)) = (&st.input, &st.output) {
        // what the engine knows: both blocks and both devices' own latency (the converters' few samples aside)
        let ms = (i.block as f64 / i.rate as f64 + o.block as f64 / o.rate as f64 + i.latency + o.latency)
            * 1000.0;
        println!("round trip, at least: {ms:.1} ms");
    }
    let block_s = st
        .input
        .as_ref()
        .map(|i| i.block as f64 / i.rate.max(1) as f64)
        .unwrap_or(0.01);
    println!("board: {}", presets[at].0);
    if let Some(w) = a.wait {
        println!("waiting for the guitar (up to {w:.0} s)...");
        let start = Instant::now();
        while start.elapsed().as_secs_f64() < w && e.meters().peak < 0.01 {
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Some(c) = a.cycle {
            println!("playing: each starting board for {c:.0} s");
        }
    }
    e.reset_stats();
    let ring = e.capture();
    // (half a second before now, so the first note is all there)
    let mut next = ring.written().saturating_sub(ring.rate() as u64 / 2);
    let mut rec: Vec<f32> = Vec::new();
    let mut lost = 0u64;
    let t0 = Instant::now();
    let mut switched = t0;
    let mut printed = t0;
    let (mut in_max, mut board_max) = (0.0f32, 0.0f32);
    let mut ch_max = [0.0f32; qrs_audio::rt::MAX_IN_CHANNELS];
    let mut sweeps = 0u32;
    while t0.elapsed().as_secs_f64() < a.seconds {
        std::thread::sleep(Duration::from_millis(40));
        if a.record.is_some() {
            let w = ring.written();
            if w > next {
                if w - next > ring.window() {
                    lost += w - next - ring.window();
                    next = w - ring.window();
                }
                let mut buf = vec![0.0f32; (w - next) as usize];
                if ring.read(next, &mut buf) {
                    rec.extend_from_slice(&buf);
                } else {
                    lost += w - next;
                }
                next = w;
            }
        }
        if let Some(c) = a.cycle
            && switched.elapsed().as_secs_f64() >= c
        {
            at = (at + 1) % presets.len();
            board_now = presets[at].1.renewed();
            e.set_tone(ToneSpec::Board {
                board: board_now.clone(),
            });
            switched = Instant::now();
            println!("\n  -> {}", presets[at].0);
        }
        if a.sweep
            && let Some(i) = board_now
                .first(Kind::Amp)
                .or((!board_now.blocks.is_empty()).then_some(0))
        {
            // the first knob from its bottom to its top and back every 4 s, a new value every 40 ms
            let kd = board_now.blocks[i].def().knobs[0];
            let ph = (t0.elapsed().as_secs_f64() / 4.0).fract() as f32;
            let tri = 1.0 - (2.0 * ph - 1.0).abs();
            board_now.blocks[i].v[0] = kd.lo + (kd.hi - kd.lo) * tri;
            e.set_tone(ToneSpec::Board {
                board: board_now.clone(),
            });
            sweeps += 1;
        }
        let m = e.meters();
        in_max = in_max.max(m.peak);
        board_max = board_max.max(m.tone_peak);
        for (c, p) in ch_max.iter_mut().zip(m.channels.iter()).take(m.channel_count) {
            *c = c.max(*p);
        }
        if printed.elapsed() >= Duration::from_millis(250) {
            printed = Instant::now();
            let s = e.stats();
            print!(
                "\r  {:5.1} s   guitar {:6.1} dBFS   board out {:6.1} dBFS   dropouts {}   gaps {}   ",
                t0.elapsed().as_secs_f64(),
                db(m.peak),
                db(m.tone_peak),
                s.dropouts,
                s.monitor_gaps
            );
            let _ = std::io::stdout().flush();
        }
        for ev in audio.events() {
            println!("\n  {ev:?}");
        }
    }
    let s = e.stats();
    println!();
    println!(
        "peaks: guitar {:.1} dBFS, board out {:.1} dBFS (the board is tuned for a guitar peaking near -12)",
        db(in_max),
        db(board_max)
    );
    let n = e.meters().channel_count;
    let chans: Vec<String> = ch_max
        .iter()
        .take(n)
        .enumerate()
        .map(|(c, p)| format!("{}: {:.1} dBFS", c + 1, db(*p)))
        .collect();
    let ch = e.channel();
    println!(
        "input channels' peaks: {}; the guitar's channel: {}",
        chans.join(", "),
        if ch < 0 {
            "not found yet (all mixed)".to_string()
        } else {
            (ch + 1).to_string()
        }
    );
    println!(
        "dropouts {}, monitor gaps {}, callbacks in {} / out {}, longest callback {:.2} ms of {:.2} ms ({:.0}%){}",
        s.dropouts,
        s.monitor_gaps,
        s.in_callbacks,
        s.out_callbacks,
        s.max_work_ns as f64 / 1e6,
        block_s * 1000.0,
        s.max_work_ns as f64 / 1e9 / block_s * 100.0,
        if a.sweep {
            format!(", {sweeps} knob edits sent")
        } else {
            String::new()
        }
    );
    if let Some(path) = &a.record {
        let rate = ring.rate();
        if rate != RATE {
            println!("(recorded at {rate} Hz; the renders assume {RATE})");
        }
        write(path, &rec)?;
        let peak = rec.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        println!(
            "dry guitar: {:.1} s, peak {:.1} dBFS, {} samples lost -> {}",
            rec.len() as f64 / rate.max(1) as f64,
            db(peak),
            lost,
            path.display()
        );
        println!("  {}", summary(&rec));
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "guitar".into());
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        for (name, b) in &presets {
            let y = board::render(b, RATE, &rec);
            write(&dir.join(format!("{stem} - {name}.wav")), &y)?;
            println!("  {:<10} {}", name, summary(&y));
        }
    }
    Ok(())
}

/// A recording of your guitar through every starting board, written beside it ("<name> - <board>.wav").
fn render_guitar(path: &Path) -> anyhow::Result<()> {
    let (rate, channels, pcm) = qrs_audio::decode::read_wav_i16(&std::fs::read(path)?)?;
    if rate != RATE {
        anyhow::bail!("{} is {rate} Hz; the boards render at {RATE}", path.display());
    }
    let x: Vec<f32> = pcm
        .chunks(channels.max(1) as usize)
        .map(|f| f[0] as f32 / 32768.0)
        .collect();
    println!(
        "{}: {:.1} s  {}",
        path.display(),
        x.len() as f64 / RATE as f64,
        summary(&x)
    );
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "guitar".into());
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for (name, b) in board::presets() {
        let y = board::render(&b, RATE, &x);
        write(&dir.join(format!("{stem} - {name}.wav")), &y)?;
        println!("  {:<10} {}", name, summary(&y));
    }
    Ok(())
}
