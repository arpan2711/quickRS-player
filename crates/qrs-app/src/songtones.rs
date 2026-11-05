//! `quickrs --song-tones DIR`: how every song tone in the library plays, on our models and with NAM (P38.2), with no
//! window. Every song in the library index is read (its parts' tones, as the player gets them); each tone's gear is
//! matched (`rs_gear::match_of`: known, guessed, flat on purpose, stand-in, left out), its board is checked for an
//! amp and a cab (an electric or bass tone), and the NAM board a song opens on (`captures::auto::nam_board`, with the
//! data folder's captures) is checked for what was picked, what stayed modelled, and what a capture missing on disk
//! would leave. Writes DIR/report.md. Reads the data folder only (set `QUICKRS_DATA` to a copy to be sure).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use qrs_audio::board::rs_gear::{self, Match};
use qrs_audio::board::{Board, Kind};
use qrs_formats::Package;
use qrs_formats::package::Format;
use qrs_formats::rig::RigTone;

use crate::captures::{self, Kind as CKind};

/// One song's parts' tones: (song id, "artist - title", [(part, tone)]).
type SongTones = (String, String, Vec<(String, RigTone)>);

fn read_all(songs: &[qrs_library::scan::SongEntry], keys: &qrs_formats::Keys) -> Vec<SongTones> {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<SongTones>> = Mutex::new(Vec::new());
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(12);
    std::thread::scope(|s| {
        for _ in 0..jobs {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(e) = songs.get(i) else { break };
                    let key = (!e.song_key.is_empty()).then_some(e.song_key.as_str());
                    let Ok(mut pkg) = Package::open(Path::new(&e.path), keys, key) else {
                        continue;
                    };
                    let mut tones = Vec::new();
                    for info in pkg.arrangements.clone() {
                        let rig = if !info.rig.is_empty() {
                            info.rig.clone()
                        } else if pkg.format() == Format::Feedpak {
                            pkg.load_info(&info, 1.0).map(|a| a.rig).unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        for t in rig {
                            tones.push((info.name.clone(), t));
                        }
                    }
                    let id = format!("{}|{}", e.path, e.song_key);
                    let name = format!("{} - {}", e.artist, e.title);
                    out.lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push((id, name, tones));
                }
            });
        }
    });
    out.into_inner().unwrap_or_else(|p| p.into_inner())
}

/// What a tone is for: "bass", "acoustic" or "electric".
pub fn tone_kind(t: &RigTone) -> &'static str {
    if rs_gear::is_bass_tone(t) {
        "bass"
    } else if t.has("ACOUSTIC") {
        "acoustic"
    } else {
        "electric"
    }
}

/// Whether a board plays an amp and a cab (or a capture with its cab in it), as an electric or bass tone needs.
pub fn amp_and_cab(b: &Board) -> (bool, bool) {
    let on = |k: Kind| b.blocks.iter().any(|x| x.on && x.def().kind == k);
    let cab_in = b.blocks.iter().any(|x| {
        x.on && x.def().id == "amp_nam" && crate::player::board::captures::has_cab(x.asset) == Some(true)
    });
    (on(Kind::Amp), on(Kind::Cab) || cab_in)
}

#[derive(Default)]
struct KeyStat {
    tones: usize,
    songs: BTreeSet<String>,
    slot: String,
}

pub fn run(out: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    let data = qrs_library::paths::DataDir::locate();
    let config = qrs_library::Config::load(data.config());
    let (keys, _) = crate::keyfile::load(&crate::keyfile::candidates(&data.root, &[]));
    captures::init_quiet(&data.root, &config);
    captures::ready();
    let mut songs: Vec<qrs_library::scan::SongEntry> = qrs_library::scan::load_index(&data.index())
        .into_values()
        .flatten()
        .filter(|e| e.error.is_empty() && !e.needs_keys)
        .collect();
    songs.sort_by(|a, b| a.path.cmp(&b.path).then(a.song_key.cmp(&b.song_key)));
    songs.dedup_by(|a, b| a.path == b.path && a.song_key == b.song_key);
    // (a song converted to a feedpak is in the index twice: counted once, its psarc first)
    let entries = songs.len();
    let mut seen_song = HashSet::new();
    songs.sort_by_key(|e| e.format == "feedpak");
    songs.retain(|e| seen_song.insert(format!("{}|{}", e.artist.to_lowercase(), e.title.to_lowercase())));
    println!("song-tones: {} songs, reading their tones", songs.len());
    let all = read_all(&songs, &keys);
    let lib_items = captures::with(|l| l.items.clone());
    let missing_files: Vec<String> = lib_items
        .iter()
        .filter(|i| !qrs_audio::board::nam::is_registered(i.id))
        .map(|i| format!("#{} {}", i.id, i.name))
        .collect();

    let mut md = String::new();
    let (mut part_tones, mut with_tones) = (0usize, 0usize);
    let mut by_match: BTreeMap<Match, usize> = BTreeMap::new();
    let mut gear_n = 0usize;
    let mut keyed: BTreeMap<(Match, String), KeyStat> = BTreeMap::new();
    // per tone kind: (tones, fully known, worst guessed, has stand-in, has left out, amp+cab ok)
    let mut kinds: BTreeMap<&str, [usize; 6]> = BTreeMap::new();
    let mut no_amp_cab: Vec<String> = Vec::new();
    // NAM
    let (mut nam_amp, mut nam_pedal, mut nam_ir, mut nam_any) = (0usize, 0usize, 0usize, 0usize);
    let (mut nam_lost, mut nam_missing, mut nam_flip_ok) = (0usize, 0usize, 0usize);
    let mut nam_lost_ex: Vec<String> = Vec::new();
    let (mut songs_nam_all, mut songs_nam_some, mut songs_modelled) = (0usize, 0usize, 0usize);
    let mut seen_tone: HashSet<String> = HashSet::new();
    let mut distinct = 0usize;
    for (id, name, tones) in &all {
        if tones.is_empty() {
            continue;
        }
        with_tones += 1;
        let (mut s_nam, mut s_amp_tones) = (0usize, 0usize);
        for (part, t) in tones {
            part_tones += 1;
            if seen_tone.insert(format!("{id}|{part}|{}", t.key)) {
                distinct += 1;
            }
            let kind = tone_kind(t);
            let k = kinds.entry(kind).or_default();
            k[0] += 1;
            let mut stand = false;
            let mut left = false;
            let mut clean = true;
            for g in &t.gear {
                let m = rs_gear::match_of(g, t);
                gear_n += 1;
                *by_match.entry(m).or_default() += 1;
                stand |= m == Match::StandIn;
                left |= m == Match::LeftOut;
                clean &= matches!(m, Match::Known | Match::Flat);
                if m != Match::Known {
                    let st = keyed.entry((m, g.key.clone())).or_default();
                    st.tones += 1;
                    st.songs.insert(id.clone());
                    st.slot = g.slot.clone();
                }
            }
            if clean {
                k[1] += 1;
            }
            if t.gear.iter().any(|g| rs_gear::match_of(g, t) == Match::Guessed) {
                k[2] += 1;
            }
            if stand {
                k[3] += 1;
            }
            if left {
                k[4] += 1;
            }
            let ours = rs_gear::board_for(t);
            let (a, c) = amp_and_cab(&ours);
            if kind == "acoustic" || (a && c) {
                k[5] += 1;
            } else if no_amp_cab.len() < 25 {
                let gear: Vec<&str> = t.gear.iter().map(|g| g.key.as_str()).collect();
                no_amp_cab.push(format!(
                    "{name} / {part} / {} ({kind}; amp {a}, cab {c}; {})",
                    t.key,
                    gear.join(" > ")
                ));
            }
            // NAM
            let nb = captures::with(|l| captures::auto::nam_board(t, l));
            let on_file = |m: &str| nb.blocks.iter().any(|x| x.on && x.def().id == m);
            let am = on_file("amp_nam");
            if am {
                nam_amp += 1;
            }
            if on_file("pedal_nam") {
                nam_pedal += 1;
            }
            if on_file("cab_ir") {
                nam_ir += 1;
            }
            if nb.blocks.iter().any(|x| x.on && x.def().file) {
                nam_any += 1;
            }
            if kind != "acoustic" {
                s_amp_tones += 1;
                if am {
                    s_nam += 1;
                }
                let (na, nc) = amp_and_cab(&nb);
                if a && c && !(na && nc) {
                    nam_lost += 1;
                    if nam_lost_ex.len() < 10 {
                        nam_lost_ex.push(format!("{name} / {part} / {}", t.key));
                    }
                }
                // flipping the amp back to MODELLED keeps an amp and a cab (the song's own model there to flip to)
                let ours_amp = ours
                    .blocks
                    .iter()
                    .find(|x| x.on && x.def().kind == Kind::Amp)
                    .map(|x| x.model);
                let flip_ok = ours_amp.is_some_and(|m| nb.blocks.iter().any(|x| x.model == m))
                    && (!am
                        || nb
                            .blocks
                            .iter()
                            .any(|x| x.def().kind == Kind::Cab && !x.def().file));
                if am && flip_ok {
                    nam_flip_ok += 1;
                }
            }
            if nb
                .blocks
                .iter()
                .any(|x| x.on && x.def().file && !qrs_audio::board::nam::is_registered(x.asset))
            {
                nam_missing += 1;
            }
        }
        if s_amp_tones > 0 && s_nam == s_amp_tones {
            songs_nam_all += 1;
        } else if s_nam > 0 {
            songs_nam_some += 1;
        } else {
            songs_modelled += 1;
        }
    }
    let pc = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    let _ = writeln!(
        md,
        "# Song tones: our models and NAM\n\n{} songs ({entries} index entries: a converted feedpak counts once); {} \
         with tones ({} parts' tones, {} distinct); {} gear pieces.\n",
        songs.len(),
        with_tones,
        part_tones,
        distinct,
        gear_n
    );
    let _ = writeln!(
        md,
        "## Gear matched\n\n| match | gear pieces | share |\n|---|---|---|"
    );
    for (m, n) in &by_match {
        let _ = writeln!(md, "| {} | {n} | {:.1}% |", m.label(), pc(*n, gear_n));
    }
    let _ = writeln!(
        md,
        "\n## Tones\n\n| tone kind | tones | every piece known (or flat) | some guessed | a stand-in | something left \
         out | amp + cab (electric, bass) |\n|---|---|---|---|---|---|---|"
    );
    for (k, v) in &kinds {
        let _ = writeln!(
            md,
            "| {k} | {} | {} ({:.1}%) | {} | {} | {} | {} ({:.1}%) |",
            v[0],
            v[1],
            pc(v[1], v[0]),
            v[2],
            v[3],
            v[4],
            v[5],
            pc(v[5], v[0])
        );
    }
    if !no_amp_cab.is_empty() {
        let _ = writeln!(md, "\nTones without an amp and a cab on our models:\n");
        for x in &no_amp_cab {
            let _ = writeln!(md, "- {x}");
        }
    }
    for m in [Match::LeftOut, Match::Guessed, Match::StandIn, Match::Flat] {
        let mut rows: Vec<(&String, &KeyStat)> = keyed
            .iter()
            .filter(|((mm, _), _)| *mm == m)
            .map(|((_, k), s)| (k, s))
            .collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by(|a, b| b.1.songs.len().cmp(&a.1.songs.len()).then(a.0.cmp(b.0)));
        let _ = writeln!(
            md,
            "\n## Gear {} ({} keys; top 30 by songs)\n\n| gear key | slot | songs | tones | stands for |\n|---|---|---|---|---|",
            m.label(),
            rows.len()
        );
        for (k, s) in rows.iter().take(30) {
            let _ = writeln!(
                md,
                "| {k} | {} | {} | {} | {} |",
                s.slot,
                s.songs.len(),
                s.tones,
                rs_gear::real_gear(k).unwrap_or("-")
            );
        }
    }
    let n = |k: CKind| lib_items.iter().filter(|i| i.kind == k).count();
    let cab_meta = |c: Option<bool>| {
        lib_items
            .iter()
            .filter(|i| i.kind == CKind::Nam && i.cab == c)
            .count()
    };
    let _ = writeln!(
        md,
        "\n## NAM (songs use NAM: {}; the data folder's captures)\n\nLibrary: {} amp captures ({} with their cab, {} \
         without, {} not saying), {} pedal captures, {} IRs; {} files not loadable.\n",
        captures::songs_use_nam(),
        n(CKind::Nam),
        cab_meta(Some(true)),
        cab_meta(Some(false)),
        cab_meta(None),
        n(CKind::Pedal),
        n(CKind::Ir),
        missing_files.len()
    );
    let _ = writeln!(
        md,
        "| on the NAM board | tones | share of all |\n|---|---|---|\n| an amp capture | {nam_amp} | {:.1}% |\n| a pedal \
         capture | {nam_pedal} | {:.1}% |\n| an IR | {nam_ir} | {:.1}% |\n| any capture | {nam_any} | {:.1}% |\n| \
         amp captured, the song's own model kept to flip to | {nam_flip_ok} | {:.1}% of amp-captured |\n| lost the amp \
         or cab ours had | {nam_lost} | |\n| a capture picked whose file isn't there | {nam_missing} | |\n",
        pc(nam_amp, part_tones),
        pc(nam_pedal, part_tones),
        pc(nam_ir, part_tones),
        pc(nam_any, part_tones),
        pc(nam_flip_ok, nam_amp),
    );
    let _ = writeln!(
        md,
        "Songs (electric and bass tones): every tone on a NAM amp {songs_nam_all}, some {songs_nam_some}, modelled only \
         {songs_modelled}.\n"
    );
    for x in &nam_lost_ex {
        let _ = writeln!(md, "- lost: {x}");
    }
    for x in missing_files.iter().take(20) {
        let _ = writeln!(md, "- not loadable: {x}");
    }
    std::fs::write(out.join("report.md"), &md)?;
    print!("{md}");
    println!("-> {}", out.join("report.md").display());
    Ok(())
}
