//! A whole folder of .psarc songs converted to feedpaks (quickrs-cli `convert-folder`, the converter window): every
//! .psarc under it (subfolders too, every song of a multi-song pack) to one .feedpak each in another folder. Songs
//! already there are skipped, so it can run again after adding songs; one that fails is reported and the rest carry
//! on.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use qrs_formats::{Keys, Package};

/// What happened to one song (sent as it happens, from the worker threads).
#[derive(Clone, Debug)]
pub enum Event {
    /// The .psarc files found, before any is converted.
    Found(usize),
    /// (files done so far, the feedpak's file name)
    Made(usize, String),
    Skipped(usize, String),
    /// (files done so far, the .psarc's file name, why)
    Failed(usize, String, String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub files: usize,
    pub made: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Convert every .psarc under `from` into `to` (made if needed). `dir`: write the folder form; `force`: convert
/// again songs already there; `stop`: set it to stop after the songs being converted.
pub fn convert_folder(
    keys: &Keys,
    from: &Path,
    to: &Path,
    dir: bool,
    force: bool,
    stop: &AtomicBool,
    on: &(dyn Fn(Event) + Sync),
) -> Result<Summary, String> {
    if !from.is_dir() {
        return Err(format!("{} isn't a folder", from.display()));
    }
    let files: Vec<PathBuf> = crate::scan::find_song_files(&[from.to_path_buf()])
        .into_iter()
        .filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("psarc")))
        .collect();
    if files.is_empty() {
        return Err(format!("no .psarc files under {}", from.display()));
    }
    std::fs::create_dir_all(to).map_err(|e| format!("couldn't make {}: {e}", to.display()))?;
    on(Event::Found(files.len()));
    let converter = format!("quickRS {}", env!("CARGO_PKG_VERSION"));
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let sum = Mutex::new(Summary {
        files: files.len(),
        ..Default::default()
    });
    let taken = Mutex::new(HashSet::<PathBuf>::new());
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    let lock = |m: &Mutex<Summary>| m.lock().unwrap_or_else(|e| e.into_inner()).to_owned();
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| {
                while !stop.load(Ordering::Relaxed) {
                    let i = next.fetch_add(1, Ordering::AcqRel);
                    let Some(f) = files.get(i) else { break };
                    let name = f
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let songs: Vec<Option<String>> = match Package::open(f, keys, None) {
                        Ok(p) if p.song_keys().len() > 1 => p.song_keys().into_iter().map(Some).collect(),
                        Ok(_) => vec![None],
                        Err(e) => {
                            let n = done.fetch_add(1, Ordering::AcqRel) + 1;
                            sum.lock().unwrap_or_else(|e| e.into_inner()).failed += 1;
                            on(Event::Failed(n, name, e.to_string()));
                            continue;
                        }
                    };
                    let mut results = Vec::new();
                    for sk in songs {
                        let base = super::default_out(f, keys, sk.as_deref());
                        let stem = base
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        // the same artist and title from two files: the second gets " (2)"
                        let target = {
                            let mut t = taken.lock().unwrap_or_else(|e| e.into_inner());
                            let mut n = 1;
                            loop {
                                let file = if n == 1 {
                                    format!("{stem}.feedpak")
                                } else {
                                    format!("{stem} ({n}).feedpak")
                                };
                                let path = to.join(file);
                                if t.insert(path.clone()) {
                                    break path;
                                }
                                n += 1;
                            }
                        };
                        let shown = target
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if target.exists() && !force {
                            results.push(Ok((shown, false)));
                            continue;
                        }
                        match super::export(f, keys, &target, sk.as_deref(), dir, &converter) {
                            Ok(_) => results.push(Ok((shown, true))),
                            Err(e) => results.push(Err(e.to_string())),
                        }
                    }
                    let n = done.fetch_add(1, Ordering::AcqRel) + 1;
                    for r in results {
                        let mut s = sum.lock().unwrap_or_else(|e| e.into_inner());
                        match r {
                            Ok((shown, true)) => {
                                s.made += 1;
                                drop(s);
                                on(Event::Made(n, shown));
                            }
                            Ok((shown, false)) => {
                                s.skipped += 1;
                                drop(s);
                                on(Event::Skipped(n, shown));
                            }
                            Err(e) => {
                                s.failed += 1;
                                drop(s);
                                on(Event::Failed(n, name.clone(), e));
                            }
                        }
                    }
                }
            });
        }
    });
    Ok(lock(&sum))
}
