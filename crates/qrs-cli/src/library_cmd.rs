//! `quickrs-cli library`: scan the library folders (psarc and feedpak) like the app does and report what it found,
//! how the duplicates group, and how long it took (first scan and from the index).

use std::path::PathBuf;
use std::time::Instant;

use qrs_formats::Keys;
use qrs_library::{Config, Library, Prefer, config, group};

#[derive(clap::Args)]
pub struct Args {
    /// config.json with library_dirs (default: the app's own)
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Song folders (instead of the config's)
    #[arg(long = "dir")]
    pub dirs: Vec<PathBuf>,
    /// Where to keep the index (default: a temporary folder, so the app's isn't touched)
    #[arg(long)]
    pub index: Option<PathBuf>,
    /// Prefer feedpaks over psarcs
    #[arg(long)]
    pub prefer_feedpak: bool,
    /// List every group with more than one source
    #[arg(long)]
    pub list: bool,
}

pub fn run(keys: &Keys, a: Args) -> anyhow::Result<()> {
    let mut dirs = a.dirs.clone();
    if dirs.is_empty() {
        let cfg = match &a.config {
            Some(p) => Config::load(p),
            None => Config::load(qrs_library::DataDir::locate().config()),
        };
        dirs = cfg.library_dirs();
    }
    for d in dirs.clone() {
        if let Some(f) = config::sibling_feedpak_dir(&d)
            && !dirs.contains(&f)
        {
            dirs.push(f);
        }
    }
    println!("folders:");
    for d in &dirs {
        println!("  {}", d.display());
    }
    let index = a
        .index
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join(format!("quickrs-library-{}.json", std::process::id())));
    let lib = Library::new();
    let t = Instant::now();
    lib.scan(keys, &dirs, &index);
    let first = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let again = Library::new();
    again.scan(keys, &dirs, &index);
    let cached = t.elapsed().as_secs_f64();
    let songs = lib.snapshot();
    let prefer = if a.prefer_feedpak {
        Prefer::Feedpak
    } else {
        Prefer::Psarc
    };
    let groups = group(&songs, prefer);
    let files = lib.total.load(std::sync::atomic::Ordering::Relaxed);
    let psarcs = songs.iter().filter(|s| !s.is_feedpak()).count();
    let errors: Vec<_> = songs.iter().filter(|s| !s.error.is_empty()).collect();
    let needs_keys = songs.iter().filter(|s| s.needs_keys).count();
    let both = groups
        .iter()
        .filter(|g| g.has_psarc(&songs) && g.has_feedpak(&songs))
        .count();
    let by_provenance = groups
        .iter()
        .filter(|g| {
            g.members.iter().any(|&i| {
                songs[i].is_feedpak()
                    && !songs[i].source_id.is_empty()
                    && g.members.iter().any(|&j| songs[j].id() == songs[i].source_id)
            })
        })
        .count();
    println!(
        "files {files}, songs {} ({psarcs} psarc, {} feedpak), errors {} (needs keys {needs_keys})",
        songs.len(),
        songs.len() - psarcs,
        errors.len()
    );
    println!(
        "rows {}: both sources {both} (by provenance {by_provenance}), psarc only {}, feedpak only {}",
        groups.len(),
        groups
            .iter()
            .filter(|g| g.has_psarc(&songs) && !g.has_feedpak(&songs))
            .count(),
        groups
            .iter()
            .filter(|g| !g.has_psarc(&songs) && g.has_feedpak(&songs))
            .count()
    );
    println!("scan {first:.2} s, again from the index {cached:.3} s");
    for e in errors.iter().take(10) {
        println!("  error: {}: {}", e.path, e.error);
    }
    if a.list {
        for g in groups.iter().filter(|g| g.members.len() > 1) {
            let names: Vec<String> = g.members.iter().map(|&i| songs[i].id()).collect();
            println!("  {} <- {}", g.canonical, names.join(" + "));
        }
    }
    if a.index.is_none() {
        let _ = std::fs::remove_file(&index);
    }
    Ok(())
}
