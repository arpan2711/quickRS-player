//! quickRS command line. Port of rsplay/cli.py, for psarc and feedpak songs alike.
//!
//!   quickrs-cli info song.psarc|song.feedpak [--files]
//!   quickrs-cli extract song.psarc out/ [--pattern GLOB] [--unpack-sng]
//!   quickrs-cli tab song --arr lead [-o tab.txt] [--width 100] [--resolution 4]
//!   quickrs-cli json song --arr lead [-o chart.json] [--indent 2]
//!
//!   quickrs-cli feedpak song.psarc [-o OUT.feedpak] [--song-key KEY] [--dir]
//!
//! tab and json also accept a loose .sng or .xml arrangement file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use qrs_formats::{Keys, Package, ascii_tab, load_keys, package::Format, psarc::Psarc, sng};

mod audio_cmds;
mod library_cmd;
#[cfg(feature = "nux")]
mod mg300_cmd;
mod verify;

#[derive(Parser)]
#[command(
    name = "quickrs-cli",
    version,
    about = "quickRS song toolkit: Rocksmith 2014 psarc and feedpak songs."
)]
struct Cli {
    /// keys.json to use (default: $RSPLAY_KEYS_FILE, ./keys.json, next to the program, ~/.config/rsplay/keys.json)
    #[arg(long, global = true, value_name = "PATH")]
    keys: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List a song's parts, audio and (optionally) every file
    Info {
        file: PathBuf,
        /// also list every file in the package
        #[arg(long)]
        files: bool,
    },
    /// Extract files from a .psarc
    Extract {
        file: PathBuf,
        out_dir: PathBuf,
        /// only extract names matching this glob, e.g. 'songs/*'
        #[arg(long)]
        pattern: Option<String>,
        /// also write decrypted, decompressed *.sng.plain
        #[arg(long)]
        unpack_sng: bool,
    },
    /// Export ASCII tab
    Tab {
        /// .psarc, .feedpak, .sng or .xml
        file: PathBuf,
        /// part: lead, rhythm, bass, combo, a number, or a prefix
        #[arg(long)]
        arr: Option<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// maximum line width
        #[arg(long, default_value_t = 100)]
        width: usize,
        /// grid slots per beat (4 = sixteenths)
        #[arg(long, default_value_t = 4, value_parser = resolution)]
        resolution: usize,
        /// difficulty between 0 and 1 (1 = the full chart)
        #[arg(long, default_value_t = 1.0)]
        mastery: f64,
    },
    /// Export the note chart as JSON
    Json {
        file: PathBuf,
        #[arg(long)]
        arr: Option<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// pretty-print with this indent
        #[arg(long)]
        indent: Option<usize>,
        #[arg(long, default_value_t = 1.0)]
        mastery: f64,
    },
    /// Write a song's album cover as PNG
    Cover {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// 128 or 256 pixels
        #[arg(long, default_value_t = 128)]
        size: u32,
        /// one song of a multi-song pack (songs.psarc)
        #[arg(long)]
        song_key: Option<String>,
    },
    /// Write a song's audio (or its preview clip) as Ogg (psarc: rebuilt losslessly from the .wem) or as WAV
    Audio {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        preview: bool,
        #[arg(long)]
        song_key: Option<String>,
    },
    /// List audio devices, ranked for the guitar, with the automatic choice (and the Rocksmith setup it used)
    Devices(audio_cmds::DeviceArgs),
    /// Play a song through the audio engine (with the guitar monitored)
    Play(audio_cmds::PlayArgs),
    /// Hear a passage of a part on the synth guitar (or write it as a WAV)
    Synth(audio_cmds::SynthArgs),
    /// Scan the library folders like the app (psarc and feedpak), group duplicates, report
    Library(library_cmd::Args),
    /// The NUX MG-300 MKII: status, backup, decode a backup, switch preset, restore
    #[cfg(feature = "nux")]
    #[command(subcommand)]
    Mg300(mg300_cmd::Cmd),
    /// Convert one .psarc song to a feedpak (spec 1.19.0): every part with its difficulty ladder, the audio
    /// rewrapped losslessly, preview, lyrics, cover. Port of `python -m rsfeedpak convert`.
    Feedpak {
        file: PathBuf,
        /// output .feedpak (default: 'Artist - Title.feedpak' in the current folder)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// which song, for multi-song packs like songs.psarc
        #[arg(long)]
        song_key: Option<String>,
        /// write the folder form instead of a zip
        #[arg(long)]
        dir: bool,
    },
    /// Convert every .psarc song under a folder (its subfolders too, every song of a multi-song pack) to
    /// feedpaks in another folder. Songs already converted there are skipped, so it can be run again after
    /// adding songs; one that fails is reported and the rest carry on.
    ConvertFolder {
        /// the folder with the .psarc files (Rocksmith's dlc folder, say)
        from: PathBuf,
        /// where the .feedpak files go (made if needed)
        to: PathBuf,
        /// write each song's folder form instead of a .feedpak zip
        #[arg(long)]
        dir: bool,
        /// convert again songs that are already there
        #[arg(long)]
        force: bool,
    },
    /// Load every song in the library folders (psarc and feedpak), every part at several difficulties, and
    /// report problems. With --python, also compare each chart with the Python reference's output.
    VerifyLibrary(verify::Args),
}

fn resolution(s: &str) -> Result<usize, String> {
    match s.parse::<usize>() {
        Ok(n) if [1, 2, 3, 4, 6, 8].contains(&n) => Ok(n),
        _ => Err("choose 1, 2, 3, 4, 6 or 8".into()),
    }
}

fn exe_dir() -> Vec<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .into_iter()
        .collect()
}

pub fn keys(path: Option<&Path>) -> anyhow::Result<Keys> {
    Ok(load_keys(path, &exe_dir())?)
}

fn size(n: u64) -> String {
    let mut x = n as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if x < 1024.0 || unit == "GB" {
            return if unit == "B" {
                format!("{x:.0} {unit}")
            } else {
                format!("{x:.1} {unit}")
            };
        }
        x /= 1024.0;
    }
    n.to_string()
}

fn duration(s: f64) -> String {
    format!("{}:{:02}", (s / 60.0) as i64, (s % 60.0) as i64)
}

fn write_out(text: &str, out: Option<&Path>) -> anyhow::Result<()> {
    match out {
        Some(p) => {
            std::fs::write(p, text)?;
            eprintln!("Wrote {}", p.display());
        }
        None => {
            let mut so = std::io::stdout().lock();
            if so.write_all(text.as_bytes()).is_err() {
                return Ok(()); // broken pipe
            }
        }
    }
    Ok(())
}

fn cmd_info(keys: &Keys, file: &Path, files: bool) -> anyhow::Result<()> {
    let mut pkg = Package::open(file, keys, None)?;
    let head: Vec<String> = [pkg.title(), pkg.artist()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    println!(
        "{}",
        if head.is_empty() {
            file.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            head.join(" - ")
        }
    );
    if !pkg.album().is_empty() {
        println!("Album: {}", pkg.album());
    }
    if pkg.year() != 0 {
        println!("Year: {}", pkg.year());
    }
    if pkg.duration() > 0.0 {
        println!("Length: {}", duration(pkg.duration()));
    }
    match pkg.format() {
        Format::Psarc => {
            let a = pkg.psarc_ref().unwrap();
            println!(
                "Archive: PSARC v{}.{}, {} files, TOC {}{}",
                a.version.0,
                a.version.1,
                a.entries.len(),
                if a.encrypted_toc { "encrypted" } else { "plain" },
                if a.platform == qrs_formats::psarc::Platform::Mac {
                    ", Mac package"
                } else {
                    ""
                }
            );
            let keys = pkg.song_keys();
            if keys.len() > 1 {
                println!("Songs in this pack: {}", keys.join(", "));
            }
        }
        Format::Feedpak => {
            let f = pkg.feedpak_ref().unwrap();
            println!(
                "Package: feedpak {}{}",
                f.version,
                if f.path.is_dir() { " (folder)" } else { "" }
            );
            if !f.source_id().is_empty() {
                println!("Converted from: {}", f.source_id());
            }
        }
    }
    println!("\nArrangements:");
    if pkg.arrangements.is_empty() {
        println!("  (none found)");
    }
    for (i, a) in pkg.arrangements.iter().enumerate() {
        let mut tmp = qrs_formats::Arrangement::new(&a.name);
        tmp.tuning = a.tuning.clone();
        tmp.is_bass = a.is_bass;
        let capo = if a.capo != 0 {
            format!(", capo {}", a.capo)
        } else {
            String::new()
        };
        let bonus = if a.bonus { " [bonus]" } else { "" };
        println!(
            "  {}. {:<8} {}{bonus}: {} ({}){capo}  <{}>",
            i + 1,
            a.key,
            a.name,
            tmp.tuning_name(),
            tmp.string_names().join(" "),
            a.chart.describe()
        );
    }
    let audio = pkg.audio_files();
    if !audio.is_empty() {
        println!("\nAudio:");
        for (i, (name, n)) in audio.iter().enumerate() {
            let role = match (pkg.format(), i) {
                (Format::Psarc, 0) => "  (full song)",
                (Format::Psarc, 1) => "  (preview)",
                _ => "",
            };
            println!("  {name}  {}{role}", size(*n));
        }
    }
    if files {
        println!("\nFiles:");
        if let Some(a) = pkg.psarc_ref() {
            for e in &a.entries {
                println!("  {:>10}  {}", e.length, e.raw_name);
            }
        }
    }
    Ok(())
}

fn cmd_extract(
    keys: &Keys,
    file: &Path,
    out: &Path,
    pattern: Option<&str>,
    unpack: bool,
) -> anyhow::Result<()> {
    let mut a = Psarc::open(file, keys.psarc.as_ref())?;
    let written = a.extract(out, pattern)?;
    let mut unpacked = 0;
    if unpack {
        for p in &written {
            if p.extension()
                .map(|e| e.eq_ignore_ascii_case("sng"))
                .unwrap_or(false)
            {
                let data = std::fs::read(p)?;
                if sng::is_packed_sng(&data) {
                    let plain = sng::decrypt_chart(&data, keys, a.platform)?;
                    let mut target = p.clone().into_os_string();
                    target.push(".plain");
                    std::fs::write(PathBuf::from(target), plain)?;
                    unpacked += 1;
                }
            }
        }
    }
    let extra = if unpacked > 0 {
        format!("; unpacked {unpacked} SNG charts (*.sng.plain)")
    } else {
        String::new()
    };
    println!("Extracted {} files to {}{extra}", written.len(), out.display());
    Ok(())
}

fn load(
    keys: &Keys,
    file: &Path,
    arr: Option<&str>,
    mastery: f64,
) -> anyhow::Result<qrs_formats::Arrangement> {
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if matches!(ext.as_str(), "psarc" | "feedpak" | "sloppak") || file.is_dir() {
        return Ok(Package::open(file, keys, None)?.load(arr, mastery)?);
    }
    Ok(qrs_formats::load_arrangement(file, arr, keys)?)
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let keys = keys(cli.keys.as_deref())?;
    match cli.command {
        Cmd::Info { file, files } => cmd_info(&keys, &file, files),
        Cmd::Feedpak {
            file,
            output,
            song_key,
            dir,
        } => cmd_feedpak(&keys, &file, output, song_key.as_deref(), dir),
        Cmd::ConvertFolder { from, to, dir, force } => cmd_convert_folder(&keys, &from, &to, dir, force),
        Cmd::Extract {
            file,
            out_dir,
            pattern,
            unpack_sng,
        } => cmd_extract(&keys, &file, &out_dir, pattern.as_deref(), unpack_sng),
        Cmd::Tab {
            file,
            arr,
            output,
            width,
            resolution,
            mastery,
        } => {
            let a = load(&keys, &file, arr.as_deref(), mastery)?;
            write_out(&ascii_tab::render(&a, width, resolution, true), output.as_deref())
        }
        Cmd::Json {
            file,
            arr,
            output,
            indent,
            mastery,
        } => {
            let a = load(&keys, &file, arr.as_deref(), mastery)?;
            let v = a.to_json();
            let text = match indent {
                Some(n) => {
                    let mut buf = Vec::new();
                    let ind = " ".repeat(n);
                    let fmt = serde_json::ser::PrettyFormatter::with_indent(ind.as_bytes());
                    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
                    serde::Serialize::serialize(&v, &mut ser)?;
                    String::from_utf8(buf)?
                }
                None => serde_json::to_string(&v)?,
            };
            write_out(&(text + "\n"), output.as_deref())
        }
        Cmd::Cover {
            file,
            output,
            size,
            song_key,
        } => {
            let mut pkg = Package::open(&file, &keys, song_key.as_deref())?;
            let img = qrs_library::art::cover(&mut pkg, size)
                .ok_or_else(|| anyhow::anyhow!("no album art in this song"))?;
            img.save(&output)?;
            eprintln!("Wrote {}", output.display());
            Ok(())
        }
        Cmd::Audio {
            file,
            output,
            preview,
            song_key,
        } => {
            let mut pkg = Package::open(&file, &keys, song_key.as_deref())?;
            let enc = qrs_formats::song_audio::song_audio(&mut pkg, preview)?
                .ok_or_else(|| anyhow::anyhow!("this song has no preview clip"))?;
            let ext = output
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext == "wav" {
                let pcm = qrs_audio::decode::decode(&enc)?;
                let samples: Vec<i16> = pcm
                    .samples
                    .iter()
                    .map(|&s| qrs_audio::decode::f32_to_i16(s))
                    .collect();
                qrs_audio::decode::write_wav_i16(&output, pcm.rate, pcm.channels, &samples)?;
                eprintln!(
                    "Wrote {} ({} Hz, {} ch, {:.1} s) from {}",
                    output.display(),
                    pcm.rate,
                    pcm.channels,
                    pcm.duration(),
                    enc.origin
                );
            } else {
                std::fs::write(&output, &enc.data)?;
                eprintln!(
                    "Wrote {} ({}, {} bytes) from {}",
                    output.display(),
                    enc.codec.ext(),
                    enc.data.len(),
                    enc.origin
                );
            }
            Ok(())
        }
        Cmd::Devices(args) => audio_cmds::devices(args),
        Cmd::Play(args) => audio_cmds::play(&keys, args),
        Cmd::Library(args) => library_cmd::run(&keys, args),
        #[cfg(feature = "nux")]
        Cmd::Mg300(c) => mg300_cmd::run(c),
        Cmd::Synth(args) => {
            let arr = load(&keys, &args.file, args.arr.as_deref(), args.mastery)?;
            audio_cmds::synth(&arr, args)
        }
        Cmd::VerifyLibrary(args) => verify::run(&keys, args),
    }
}

fn cmd_feedpak(
    keys: &Keys,
    file: &Path,
    output: Option<PathBuf>,
    song_key: Option<&str>,
    dir: bool,
) -> anyhow::Result<()> {
    use qrs_library::feedpak_export;
    // the current folder, not next to the source (which would put it in Rocksmith's dlc folder)
    let out = output.unwrap_or_else(|| feedpak_export::default_out(file, keys, song_key));
    let converter = format!("quickRS {}", env!("CARGO_PKG_VERSION"));
    let r = feedpak_export::export(file, keys, &out, song_key, dir, &converter)?;
    println!(
        "{} - {}: parts {}; audio {}; {:.1}s -> {}",
        r.artist,
        r.title,
        r.parts.join(", "),
        r.audio,
        r.seconds,
        out.display()
    );
    for w in &r.warnings {
        println!("  warning: {w}");
    }
    Ok(())
}

fn cmd_convert_folder(keys: &Keys, from: &Path, to: &Path, dir: bool, force: bool) -> anyhow::Result<()> {
    use qrs_library::feedpak_export::folder::{Event, convert_folder};
    let total = std::sync::atomic::AtomicUsize::new(0);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let on = |e: Event| {
        let n = total.load(std::sync::atomic::Ordering::Relaxed);
        match e {
            Event::Found(k) => {
                total.store(k, std::sync::atomic::Ordering::Relaxed);
                println!(
                    "{k} psarc files under {}; feedpaks go to {}",
                    from.display(),
                    to.display()
                );
            }
            Event::Made(i, name) => println!("[{i}/{n}] {name}"),
            Event::Skipped(..) => {}
            Event::Failed(i, name, why) => println!("[{i}/{n}] {name}: FAILED ({why})"),
        }
    };
    let s = convert_folder(keys, from, to, dir, force, &stop, &on).map_err(anyhow::Error::msg)?;
    println!(
        "done: {} converted, {} already there, {} failed",
        s.made, s.skipped, s.failed
    );
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
