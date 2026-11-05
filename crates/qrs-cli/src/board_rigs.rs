//! `quickrs-cli board rigs`: how well the song tone matcher (qrs-audio `board::rs_gear`) covers a library. Every
//! tone of every part of every song (psarc and feedpak; a tone met twice, as a feedpak converted from a psarc has
//! it, counts once) is matched gear by gear; the report lists each gear key with how many tones use it, the model it
//! becomes (or LEFT OUT), the knobs seen with their ranges, and the share of gear and of whole tones matched.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use qrs_audio::board::rs_gear;
use qrs_formats::rig::RigTone;
use qrs_formats::{Keys, Package, package::Format};

#[derive(Default)]
struct GearStat {
    tones: usize,
    /// model id (or "LEFT OUT") → tones
    models: BTreeMap<String, usize>,
    /// knob → (min, max)
    knobs: BTreeMap<String, (f32, f32)>,
    /// a few tones that use it, for looking one up
    examples: Vec<String>,
}

fn kind(slot: &str) -> &'static str {
    match slot {
        "Amp" => "amp",
        "Cabinet" => "cab",
        s if s.starts_with("Rack") => "rack",
        _ => "pedal",
    }
}

fn signature(t: &RigTone) -> String {
    let mut s = t.key.to_lowercase();
    for g in &t.gear {
        s.push_str(&format!("|{}:{}", g.slot, g.key));
        for (k, v) in &g.knobs {
            s.push_str(&format!(",{k}={v}"));
        }
    }
    s
}

/// Every tone of every part in the song files, deduplicated.
fn tones_of(keys: &Keys, files: &[PathBuf], jobs: usize) -> (Vec<RigTone>, usize, Vec<String>) {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<RigTone>> = Mutex::new(Vec::new());
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let parts = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..jobs.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let mut found: Vec<RigTone> = Vec::new();
                    let pkg = match Package::open(path, keys, None) {
                        Ok(p) => p,
                        Err(e) => {
                            errors.lock().unwrap().push(format!("{}: {e}", path.display()));
                            continue;
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
                        let Ok(mut pkg) = Package::open(path, keys, (!sk.is_empty()).then_some(sk.as_str()))
                        else {
                            continue;
                        };
                        let infos = pkg.arrangements.clone();
                        for info in &infos {
                            parts.fetch_add(1, Ordering::Relaxed);
                            if !info.rig.is_empty() {
                                found.extend(info.rig.iter().cloned());
                            } else if pkg.format() == Format::Feedpak {
                                match pkg.load_info(info, 1.0) {
                                    Ok(a) => found.extend(a.rig),
                                    Err(e) => errors.lock().unwrap().push(format!(
                                        "{} {}: {e}",
                                        path.display(),
                                        info.name
                                    )),
                                }
                            }
                        }
                    }
                    out.lock().unwrap().extend(found);
                }
            });
        }
    });
    let mut seen = HashSet::new();
    let mut tones = out.into_inner().unwrap();
    tones.retain(|t| seen.insert(signature(t)));
    tones.sort_by(|a, b| a.key.cmp(&b.key));
    (tones, parts.into_inner(), errors.into_inner().unwrap())
}

pub fn run(
    keys: &Keys,
    dirs: &[PathBuf],
    jobs: usize,
    report: Option<&std::path::Path>,
    only: Option<&str>,
) -> anyhow::Result<()> {
    let files = crate::verify::find_song_files(dirs);
    eprintln!("{} song files in {} folders", files.len(), dirs.len());
    let (tones, parts, errors) = tones_of(keys, &files, jobs);
    if let Some(o) = only {
        // every tone with this gear: its descriptors, its rig and the board it becomes
        for t in tones.iter().filter(|t| t.gear.iter().any(|g| g.key.contains(o))) {
            let gear: Vec<&str> = t.gear.iter().map(|g| g.key.as_str()).collect();
            let board: Vec<&str> = rs_gear::board_for(t).blocks.iter().map(|b| b.def().id).collect();
            println!(
                "{} {:?}\n    {}\n    -> {}",
                t.key,
                t.descriptors,
                gear.join(" > "),
                board.join(" > ")
            );
        }
        return Ok(());
    }
    let mut stats: BTreeMap<(String, String), GearStat> = BTreeMap::new();
    let (mut instances, mut mapped, mut full) = (0usize, 0usize, 0usize);
    let mut by_kind: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let (mut amps, mut amps_known) = (0usize, 0usize);
    for t in &tones {
        let mut all = true;
        for g in &t.gear {
            let id = rs_gear::blocks_for(g, t).map(|bs| {
                if bs.is_empty() {
                    "(flat)".to_string()
                } else {
                    bs.iter().map(|b| b.def().id).collect::<Vec<_>>().join("+")
                }
            });
            if g.slot == "Amp" {
                amps += 1;
                if rs_gear::amp_known(&g.key) {
                    amps_known += 1;
                }
            }
            instances += 1;
            let k = by_kind.entry(kind(&g.slot)).or_default();
            k.0 += 1;
            if id.is_some() {
                mapped += 1;
                k.1 += 1;
            } else {
                all = false;
            }
            let st = stats
                .entry((kind(&g.slot).to_string(), g.key.clone()))
                .or_default();
            st.tones += 1;
            *st.models
                .entry(id.unwrap_or_else(|| "LEFT OUT".into()))
                .or_default() += 1;
            for (name, v) in &g.knobs {
                let e = st.knobs.entry(name.clone()).or_insert((*v, *v));
                e.0 = e.0.min(*v);
                e.1 = e.1.max(*v);
            }
            if st.examples.len() < 2 {
                st.examples.push(t.key.clone());
            }
        }
        if all {
            full += 1;
        }
    }
    let pc = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    let mut r = String::new();
    r.push_str(&format!(
        "{} files, {} parts, {} distinct tones, {} distinct gear keys ({} errors)\n",
        files.len(),
        parts,
        tones.len(),
        stats.len(),
        errors.len()
    ));
    r.push_str(&format!(
        "gear instances matched: {mapped}/{instances} = {:.1}%\n",
        pc(mapped, instances)
    ));
    for (k, (n, m)) in &by_kind {
        r.push_str(&format!("  {k:<6} {m}/{n} = {:.1}%\n", pc(*m, *n)));
    }
    r.push_str(&format!(
        "amps matched by what they are (not guessed from the tone): {amps_known}/{amps} = {:.1}%\n",
        pc(amps_known, amps)
    ));
    r.push_str(&format!(
        "tones fully matched: {full}/{} = {:.1}%\n",
        tones.len(),
        pc(full, tones.len())
    ));
    let left: usize = stats
        .values()
        .filter(|s| s.models.contains_key("LEFT OUT"))
        .count();
    r.push_str(&format!("gear keys left out: {left}\n\n"));
    let mut rows: Vec<_> = stats.iter().collect();
    rows.sort_by(|a, b| {
        a.0.0
            .cmp(&b.0.0)
            .then(b.1.tones.cmp(&a.1.tones))
            .then(a.0.1.cmp(&b.0.1))
    });
    let mut last = String::new();
    for ((k, key), st) in rows {
        if *k != last {
            r.push_str(&format!("== {}\n", k.to_uppercase()));
            last = k.clone();
        }
        let models: Vec<String> = st.models.iter().map(|(m, n)| format!("{m} x{n}")).collect();
        let knobs: Vec<String> = st
            .knobs
            .iter()
            .map(|(n, (lo, hi))| {
                if lo == hi {
                    format!("{n}={lo}")
                } else {
                    format!("{n}={lo}..{hi}")
                }
            })
            .collect();
        let real = rs_gear::real_gear(key)
            .map(|r| format!("   [{r}]"))
            .unwrap_or_default();
        r.push_str(&format!(
            "{:>5}  {:<34} -> {}{real}\n       knobs: {}\n       e.g. {}\n",
            st.tones,
            key,
            models.join(", "),
            if knobs.is_empty() {
                "-".into()
            } else {
                knobs.join("  ")
            },
            st.examples.join(", ")
        ));
    }
    let set: BTreeSet<&String> = errors.iter().collect();
    if !set.is_empty() {
        r.push_str(&format!("\nerrors ({}):\n", set.len()));
        for e in set.iter().take(20) {
            r.push_str(&format!("  {e}\n"));
        }
    }
    print!("{r}");
    if let Some(p) = report {
        std::fs::write(p, &r)?;
        eprintln!("-> {}", p.display());
    }
    Ok(())
}
