//! `quickrs-cli verify-library`: load every song in the library folders (psarc and feedpak), every part at
//! several difficulties, and report problems. Optionally compares each chart with the Python reference's
//! (tools/parity/ref_charts.py) and each converted feedpak with the psarc it came from.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use qrs_formats::{Keys, Package, package::Format};
use serde_json::Value;

#[derive(clap::Args)]
pub struct Args {
    /// Library folders (default: library_dirs from --config)
    dirs: Vec<PathBuf>,
    /// config.json to read library_dirs from
    #[arg(long)]
    config: Option<PathBuf>,
    /// Compare with the Python reference: a python.exe that can import rsplay and rsfeedpak
    #[arg(long)]
    python: Option<PathBuf>,
    /// keys.json for the Python reference (default: the one quickRS found)
    #[arg(long)]
    python_keys: Option<PathBuf>,
    /// Difficulties to check, comma separated
    #[arg(long, default_value = "1,0.8,0.5,0.25,0")]
    mastery: String,
    /// Only the first N song files
    #[arg(long)]
    limit: Option<usize>,
    /// Only files whose name contains this text
    #[arg(long)]
    filter: Option<String>,
    /// Write the full report here
    #[arg(long)]
    report: Option<PathBuf>,
    /// Worker threads (each with its own Python process when --python is given)
    #[arg(long, default_value_t = 8)]
    jobs: usize,
    /// Skip comparing feedpaks with their psarc sources
    #[arg(long)]
    no_twins: bool,
    /// Also convert every song's audio (and preview); a converted feedpak's audio must be byte-identical to ours
    #[arg(long)]
    audio: bool,
}

/// Multi-song game archives that live next to the dlc folder; they aren't single songs.
const NOT_SONGS: [&str; 10] = [
    "audio", "cache", "crowd", "etudes", "gears", "guitars", "session", "songs", "static", "video",
];

pub fn find_song_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut items: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        items.sort();
        for p in items {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext == "feedpak" || ext == "sloppak" {
                out.push(p); // file or folder pack
            } else if p.is_dir() {
                walk(&p, out);
            } else if ext == "psarc" {
                let stem = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !NOT_SONGS.contains(&stem.as_str()) {
                    out.push(p);
                }
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        if !d.is_dir() {
            continue;
        }
        walk(d, &mut out);
        // The base game's on-disc songs live next to the dlc folder.
        if d.file_name()
            .map(|n| n.to_string_lossy().eq_ignore_ascii_case("dlc"))
            .unwrap_or(false)
            && let Some(parent) = d.parent()
            && parent.join("songs.psarc").is_file()
        {
            out.push(parent.join("songs.psarc"));
        }
    }
    out
}

struct Py {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Py {
    fn start(python: &Path, keys: &Path) -> anyhow::Result<Py> {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/ref_charts.py");
        let mut child = Command::new(python)
            .arg("-X")
            .arg("utf8")
            .arg(&script)
            .arg("--keys")
            .arg(keys)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Ok(Py {
            child,
            stdin,
            stdout,
            next: 0,
        })
    }

    fn chart(&mut self, path: &Path, song_key: &str, arr: &str, mastery: f64) -> Result<Value, String> {
        self.next += 1;
        let job = serde_json::json!({"id": self.next, "path": path.to_string_lossy(), "song_key": song_key, "arr": arr, "mastery": mastery});
        writeln!(self.stdin, "{job}").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&line).map_err(|e| format!("bad reply: {e}"))?;
        if let Some(e) = v.get("error") {
            return Err(e.as_str().unwrap_or("?").to_string());
        }
        Ok(v["chart"].clone())
    }
}

impl Drop for Py {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// The first place two JSON values differ, as "path: a != b".
pub fn first_diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, va) in x {
                match y.get(k) {
                    Some(vb) => {
                        if let Some(d) = first_diff(va, vb, &format!("{path}.{k}")) {
                            return Some(d);
                        }
                    }
                    None => return Some(format!("{path}.{k}: only in ours")),
                }
            }
            y.keys()
                .find(|k| !x.contains_key(*k))
                .map(|k| format!("{path}.{k}: only in theirs"))
        }
        (Value::Array(x), Value::Array(y)) => {
            for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                if let Some(d) = first_diff(va, vb, &format!("{path}[{i}]")) {
                    return Some(d);
                }
            }
            (x.len() != y.len()).then(|| format!("{path}: length {} != {}", x.len(), y.len()))
        }
        (Value::Number(x), Value::Number(y)) => {
            let same = match (x.as_f64(), y.as_f64()) {
                (Some(p), Some(q)) => p == q,
                _ => x == y,
            };
            (!same).then(|| format!("{path}: {x} != {y}"))
        }
        _ => {
            let s = |v: &Value| {
                let t = v.to_string();
                if t.len() > 80 {
                    format!("{}…", &t[..80])
                } else {
                    t
                }
            };
            (a != b).then(|| format!("{path}: {} != {}", s(a), s(b)))
        }
    }
}

#[derive(Default)]
struct Tally {
    files: usize,
    songs: usize,
    charts: usize,
    compared: usize,
    twins: usize,
    rust_errors: Vec<String>,
    python_errors: Vec<String>,
    mismatches: Vec<String>,
    twin_mismatches: Vec<String>,
    missing_twins: usize,
    audio_converted: usize,
    audio_identical: usize,
}

pub fn run(keys: &Keys, args: Args) -> anyhow::Result<()> {
    let mut dirs = args.dirs.clone();
    if dirs.is_empty() {
        let cfg_path = args
            .config
            .clone()
            .unwrap_or_else(|| PathBuf::from("config.json"));
        let cfg: Value = serde_json::from_str(
            &std::fs::read_to_string(&cfg_path)
                .map_err(|e| anyhow::anyhow!("{}: {e} (give folders or --config)", cfg_path.display()))?,
        )?;
        dirs = cfg["library_dirs"]
            .as_array()
            .map(|a| a.iter().filter_map(|d| d.as_str().map(PathBuf::from)).collect())
            .unwrap_or_default();
    }
    let masteries: Vec<f64> = args
        .mastery
        .split(',')
        .map(|s| s.trim().parse::<f64>())
        .collect::<Result<_, _>>()?;
    let mut files = find_song_files(&dirs);
    if let Some(f) = &args.filter {
        let f = f.to_lowercase();
        files.retain(|p| p.to_string_lossy().to_lowercase().contains(&f));
    }
    if let Some(n) = args.limit {
        files.truncate(n);
    }
    // psarc file name -> path, for feedpak twins (rsplay_id is "file_p.psarc|SongKey")
    let psarcs: HashMap<String, PathBuf> = find_song_files(&dirs)
        .into_iter()
        .filter(|p| {
            p.extension()
                .map(|e| e.eq_ignore_ascii_case("psarc"))
                .unwrap_or(false)
        })
        .map(|p| (p.file_name().unwrap().to_string_lossy().to_lowercase(), p))
        .collect();
    let python_keys = args.python_keys.clone().or_else(|| keys.source.clone());
    if args.python.is_some() && python_keys.is_none() {
        anyhow::bail!("--python needs a keys file (--python-keys)");
    }
    eprintln!(
        "verify-library: {} files in {} folders, mastery {:?}{}",
        files.len(),
        dirs.len(),
        masteries,
        if args.python.is_some() {
            ", comparing with Python"
        } else {
            ""
        }
    );

    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let tally = Mutex::new(Tally::default());
    std::thread::scope(|s| -> anyhow::Result<()> {
        let mut handles = Vec::new();
        for _ in 0..args.jobs.max(1) {
            handles.push(s.spawn(|| -> anyhow::Result<()> {
                let mut py = match &args.python {
                    Some(p) => Some(Py::start(p, python_keys.as_ref().unwrap())?),
                    None => None,
                };
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let mut t = Tally {
                        files: 1,
                        ..Default::default()
                    };
                    check_file(
                        path,
                        keys,
                        &masteries,
                        py.as_mut(),
                        &psarcs,
                        !args.no_twins,
                        args.audio,
                        &mut t,
                    );
                    let mut g = tally.lock().unwrap();
                    g.files += t.files;
                    g.songs += t.songs;
                    g.charts += t.charts;
                    g.compared += t.compared;
                    g.twins += t.twins;
                    g.missing_twins += t.missing_twins;
                    g.audio_converted += t.audio_converted;
                    g.audio_identical += t.audio_identical;
                    g.rust_errors.extend(t.rust_errors);
                    g.python_errors.extend(t.python_errors);
                    g.mismatches.extend(t.mismatches);
                    g.twin_mismatches.extend(t.twin_mismatches);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(50) || n == files.len() {
                        eprintln!(
                            "  {n}/{} files, {} charts, {} errors, {} mismatches, {:.0}s",
                            files.len(),
                            g.charts,
                            g.rust_errors.len(),
                            g.mismatches.len() + g.twin_mismatches.len(),
                            start.elapsed().as_secs_f64()
                        );
                    }
                }
                Ok(())
            }));
        }
        for h in handles {
            h.join().unwrap()?;
        }
        Ok(())
    })?;

    let t = tally.into_inner().unwrap();
    let mut report = String::new();
    report += &format!(
        "files {}  songs {}  charts {}  compared with Python {}  feedpak/psarc twins compared {} (no source found for {})\n",
        t.files, t.songs, t.charts, t.compared, t.twins, t.missing_twins
    );
    if t.audio_converted > 0 {
        report += &format!(
            "audio: {} songs/previews converted, {} byte-identical to their converted feedpak
",
            t.audio_converted, t.audio_identical
        );
    }
    report += &format!(
        "errors: ours {}  Python's {}   mismatches: vs Python {}  feedpak vs psarc {}\n",
        t.rust_errors.len(),
        t.python_errors.len(),
        t.mismatches.len(),
        t.twin_mismatches.len()
    );
    for (title, list) in [
        ("Our errors", &t.rust_errors),
        ("Python errors", &t.python_errors),
        ("Mismatches with Python", &t.mismatches),
        ("feedpak vs psarc mismatches", &t.twin_mismatches),
    ] {
        if !list.is_empty() {
            report += &format!("\n## {title}\n");
            let mut l = list.clone();
            l.sort();
            for x in l {
                report += &format!("- {x}\n");
            }
        }
    }
    match &args.report {
        Some(p) => {
            std::fs::write(p, &report)?;
            eprintln!("report: {}", p.display());
            print!("{}", report.lines().take(2).collect::<Vec<_>>().join("\n"));
            println!();
        }
        None => print!("{report}"),
    }
    if !t.rust_errors.is_empty() || !t.mismatches.is_empty() || !t.twin_mismatches.is_empty() {
        anyhow::bail!("verify-library found problems");
    }
    Ok(())
}

fn label(path: &Path, song_key: &str) -> String {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if song_key.is_empty() {
        name
    } else {
        format!("{name}|{song_key}")
    }
}

fn check_file(
    path: &Path,
    keys: &Keys,
    masteries: &[f64],
    mut py: Option<&mut Py>,
    psarcs: &HashMap<String, PathBuf>,
    twins: bool,
    audio: bool,
    t: &mut Tally,
) {
    let pkg = match Package::open(path, keys, None) {
        Ok(p) => p,
        Err(e) => {
            t.rust_errors.push(format!("{}: {e}", label(path, "")));
            return;
        }
    };
    let song_keys = pkg.song_keys();
    let subs: Vec<String> = if song_keys.len() <= 1 {
        vec![String::new()]
    } else {
        song_keys
    };
    drop(pkg);
    for sk in subs {
        let mut pkg = match Package::open(path, keys, (!sk.is_empty()).then_some(sk.as_str())) {
            Ok(p) => p,
            Err(e) => {
                t.rust_errors.push(format!("{}: {e}", label(path, &sk)));
                continue;
            }
        };
        t.songs += 1;
        if pkg.arrangements.is_empty() {
            t.rust_errors
                .push(format!("{}: no playable arrangements", label(path, &sk)));
            continue;
        }
        let _ = pkg
            .load_lyrics()
            .map_err(|e| t.rust_errors.push(format!("{} lyrics: {e}", label(path, &sk))));
        // the psarc this feedpak was converted from
        let mut twin: Option<Package> = None;
        if twins && pkg.format() == Format::Feedpak {
            let id = pkg.feedpak_ref().map(|f| f.source_id()).unwrap_or_default();
            if let Some((file, key)) = id.split_once('|') {
                match psarcs.get(&file.to_lowercase()) {
                    Some(p) => match Package::open(p, keys, (!key.is_empty()).then_some(key)) {
                        Ok(tw) => twin = Some(tw),
                        Err(e) => t
                            .rust_errors
                            .push(format!("{} source {file}: {e}", label(path, &sk))),
                    },
                    None => t.missing_twins += 1,
                }
            }
        }
        if audio {
            check_audio(&mut pkg, twin.as_mut(), &label(path, &sk), t);
        }
        let infos = pkg.arrangements.clone();
        for info in &infos {
            for &m in masteries {
                let ours = match pkg.load_info(info, m) {
                    Ok(a) => a.to_json(),
                    Err(e) => {
                        t.rust_errors
                            .push(format!("{} {} @{m}: {e}", label(path, &sk), info.key));
                        continue;
                    }
                };
                t.charts += 1;
                if let Some(py) = py.as_deref_mut() {
                    match py.chart(path, &sk, &info.key, m) {
                        Ok(theirs) => {
                            t.compared += 1;
                            if let Some(d) = first_diff(&ours, &theirs, "") {
                                t.mismatches
                                    .push(format!("{} {} @{m}: {d}", label(path, &sk), info.key));
                            }
                        }
                        Err(e) => {
                            t.python_errors
                                .push(format!("{} {} @{m}: {e}", label(path, &sk), info.key))
                        }
                    }
                }
                if let Some(tw) = twin.as_mut() {
                    // pair parts by the source chart the pack recorded (as rsfeedpak.verify does)
                    let tinfo = tw
                        .arrangements
                        .iter()
                        .find(|a| !info.source_chart.is_empty() && a.chart.describe() == info.source_chart)
                        .or_else(|| tw.arrangements.iter().find(|a| a.key == info.key))
                        .cloned();
                    let Some(tinfo) = tinfo else {
                        t.twin_mismatches.push(format!(
                            "{} {}: part not in the source psarc",
                            label(path, &sk),
                            info.key
                        ));
                        continue;
                    };
                    match (tw.load_info(&tinfo, m), pkg.load_info(info, m)) {
                        (Ok(src), Ok(fp)) => {
                            t.twins += 1;
                            let diffs = qrs_formats::compare::compare_arrangements(
                                &src,
                                &fp,
                                &format!("{}@{m}", info.key),
                            );
                            if let Some(d) = diffs.first() {
                                t.twin_mismatches.push(format!(
                                    "{} {d} (+{} more)",
                                    label(path, &sk),
                                    diffs.len() - 1
                                ));
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            t.rust_errors
                                .push(format!("{} source {} @{m}: {e}", label(path, &sk), info.key))
                        }
                    }
                }
            }
        }
    }
}

fn check_audio(pkg: &mut Package, twin: Option<&mut Package>, label: &str, t: &mut Tally) {
    use qrs_formats::song_audio::song_audio;
    match pkg.format() {
        Format::Psarc => {
            for preview in [false, true] {
                match song_audio(pkg, preview) {
                    Ok(Some(_)) => t.audio_converted += 1,
                    Ok(None) => {}
                    Err(e) => t.rust_errors.push(format!(
                        "{label} {}: {e}",
                        if preview { "preview" } else { "audio" }
                    )),
                }
            }
        }
        Format::Feedpak => {
            let Some(src) = twin else { return };
            for preview in [false, true] {
                let what = if preview { "preview" } else { "audio" };
                let ours = match song_audio(src, preview) {
                    Ok(Some(a)) => a,
                    Ok(None) => continue,
                    Err(e) => {
                        t.rust_errors.push(format!("{label} source {what}: {e}"));
                        continue;
                    }
                };
                t.audio_converted += 1;
                match song_audio(pkg, preview) {
                    Ok(Some(theirs)) => {
                        if theirs.data == ours.data {
                            t.audio_identical += 1;
                        } else {
                            t.twin_mismatches.push(format!(
                                "{label} {what}: {} bytes vs {} in the feedpak ({})",
                                ours.data.len(),
                                theirs.data.len(),
                                theirs.origin
                            ));
                        }
                    }
                    Ok(None) => t
                        .twin_mismatches
                        .push(format!("{label} {what}: missing in the feedpak")),
                    Err(e) => t.rust_errors.push(format!("{label} {what}: {e}")),
                }
            }
        }
    }
}
