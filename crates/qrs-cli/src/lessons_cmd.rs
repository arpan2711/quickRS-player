//! `quickrs-cli lessons check [FILES...]` (new in quickRS, P16.7): every item of the built-in lessons and the given
//! lesson files against the library, as the app would open it: the song found (which file), the part, the notes in
//! the stretch and how its start and end sit against them, the phrase found by ear, a scale's shape on the chart's
//! tuning and how much of the stretch is in it. Errors (an item the app couldn't teach) make the exit code 1;
//! warnings are for the author to look at.

use std::path::{Path, PathBuf};
use std::time::Instant;

use qrs_formats::model::note_name;
use qrs_formats::{Arrangement, Keys, Package, Techs};
use qrs_library::lessons::{self, Kind, Lesson, Riff, in_scale, pc_name};
use qrs_library::{Config, Library, Prefer, SongEntry, group};

use crate::library_cmd;

#[derive(clap::Subcommand)]
pub enum Cmd {
    /// Check lessons against the library: the built-in ones and these files
    Check(CheckArgs),
}

#[derive(clap::Args)]
pub struct CheckArgs {
    /// Lesson files (.json) to check besides the built-in lessons
    pub files: Vec<PathBuf>,
    /// Only the files given, not the built-in lessons
    #[arg(long)]
    pub no_builtin: bool,
    /// config.json with library_dirs (default: the app's own)
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Song folders (instead of the config's)
    #[arg(long = "dir")]
    pub dirs: Vec<PathBuf>,
    /// The library index to keep between runs (default: one in the temporary folder, so a second run is quick)
    #[arg(long)]
    pub index: Option<PathBuf>,
}

/// A note of a chart: time, string, fret, ring (seconds), muted, part of a chord.
#[derive(Clone, Copy, Debug)]
struct Hit {
    t: f64,
    string: i32,
    fret: i32,
    sus: f64,
    muted: bool,
    chord: bool,
}

/// Every note of a part (chords' notes too), in time order (ignored notes left out).
fn hits(arr: &Arrangement) -> Vec<Hit> {
    let mut out: Vec<Hit> = arr
        .notes
        .iter()
        .filter(|n| !n.techniques.has(Techs::IGNORE))
        .map(|n| Hit {
            t: n.time,
            string: n.string,
            fret: n.fret,
            sus: n.sustain,
            muted: n.techniques.has(Techs::FRET_HAND_MUTE),
            chord: false,
        })
        .collect();
    for c in &arr.chords {
        for n in &c.notes {
            let techs = n.techniques | c.techniques;
            out.push(Hit {
                t: n.time,
                string: n.string,
                fret: n.fret,
                sus: n.sustain.max(c.sustain),
                muted: techs.has(Techs::FRET_HAND_MUTE),
                chord: true,
            });
        }
    }
    out.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.string.cmp(&b.string)));
    out
}

/// How close (seconds) a note can be to the stretch's start and still be in it, and to its end and be out of it (the
/// coach's own margins: a time measured on a note is a hair either side of it in the chart).
const AT_START: f64 = 0.02;
const AT_END: f64 = 0.01;

/// "C#4".
fn name(midi: i32) -> String {
    format!("{}{}", note_name(midi), midi.div_euclid(12) - 1)
}

/// A string's name as the tab shows it: "A", and the high E string "e".
fn string_name(strings: &[&str], s: i32) -> String {
    let n = strings.get(s as usize).copied().unwrap_or("?");
    if s > 0 && s as usize + 1 == strings.len() && strings.first() == Some(&n) {
        n.to_lowercase()
    } else {
        n.to_string()
    }
}

/// What was found about one item: its lines, and how many errors and warnings.
#[derive(Default)]
struct Report {
    lines: Vec<String>,
    errors: usize,
    warnings: usize,
}

impl Report {
    fn info(&mut self, s: impl Into<String>) {
        self.lines.push(format!("    {}", s.into()));
    }
    fn warn(&mut self, s: impl Into<String>) {
        self.warnings += 1;
        self.lines.push(format!("    warning: {}", s.into()));
    }
    fn error(&mut self, s: impl Into<String>) {
        self.errors += 1;
        self.lines.push(format!("    ERROR: {}", s.into()));
    }
}

pub fn run(keys: &Keys, cmd: Cmd) -> anyhow::Result<()> {
    let Cmd::Check(a) = cmd;
    // the lessons
    let mut lessons: Vec<(String, Lesson)> = Vec::new();
    let mut errors = 0usize;
    if !a.no_builtin {
        for (file, l) in lessons::builtin_files() {
            match l {
                Ok(l) => lessons.push((format!("{file} (built in)"), l)),
                Err(e) => {
                    println!("{file} (built in): ERROR: {e}");
                    errors += 1;
                }
            }
        }
    }
    for f in &a.files {
        match std::fs::read_to_string(f)
            .map_err(|e| e.to_string())
            .and_then(|t| Lesson::parse(&t))
        {
            Ok(l) => lessons.push((f.display().to_string(), l)),
            Err(e) => {
                println!("{}: ERROR: {e}", f.display());
                errors += 1;
            }
        }
    }
    // lessons together: numbers within a course, what each builds on
    let all_lessons: Vec<&Lesson> = lessons.iter().map(|(_, l)| l).collect();
    let set: Vec<Lesson> = all_lessons.iter().map(|l| (*l).clone()).collect();
    for (id, why) in lessons::set_problems(&set) {
        println!("{id}: ERROR: {why}");
        errors += 1;
    }
    // the library, as the app reads it
    let dirs = library_cmd::folders(a.config.as_deref(), &a.dirs);
    let index = a
        .index
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("quickrs-lessons-check-index.json"));
    let t0 = Instant::now();
    let lib = Library::new();
    lib.scan(keys, &dirs, &index);
    let songs = lib.snapshot();
    let cfg = match &a.config {
        Some(p) => Config::load(p),
        None => Config::load(qrs_library::DataDir::locate().config()),
    };
    let prefer = Prefer::parse(&cfg.str("prefer", "")).unwrap_or(if keys.psarc.is_some() {
        Prefer::Psarc
    } else {
        Prefer::Feedpak
    });
    let groups = group(&songs, prefer);
    println!(
        "library: {} songs in {} folder{} ({:.1} s; index {})",
        songs.len(),
        dirs.len(),
        if dirs.len() == 1 { "" } else { "s" },
        t0.elapsed().as_secs_f64(),
        index.display()
    );
    let mut warnings = 0usize;
    for (file, l) in &lessons {
        println!();
        println!(
            "{}: {} \"{}\" ({}, course order {}, {} item{})",
            file,
            l.label(),
            l.title,
            if l.course.is_empty() {
                "no course"
            } else {
                &l.course
            },
            l.course_order,
            l.riffs.len(),
            if l.riffs.len() == 1 { "" } else { "s" }
        );
        if !l.builds_on.is_empty() {
            println!("  builds on: {}", l.builds_on.join(", "));
        }
        for (i, r) in l.riffs.iter().enumerate() {
            let rep = check_item(keys, &songs, &groups, r);
            println!(
                "  {}. {} [{}] {}: {} - {}, {}, {:.3}-{:.3} s",
                i + 1,
                r.id,
                r.kind.label(),
                r.name,
                r.artist,
                r.title,
                r.part,
                r.start,
                r.end
            );
            for line in &rep.lines {
                println!("{line}");
            }
            errors += rep.errors;
            warnings += rep.warnings;
        }
    }
    println!();
    println!(
        "{} lesson{}: {errors} error{}, {warnings} warning{}",
        lessons.len(),
        if lessons.len() == 1 { "" } else { "s" },
        if errors == 1 { "" } else { "s" },
        if warnings == 1 { "" } else { "s" }
    );
    if errors > 0 {
        anyhow::bail!(
            "{errors} error{} in the lessons",
            if errors == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

/// The library's song for an item: its group (as the app finds it: the canonical id, else the same artist with one
/// title inside the other) and the file the app would open.
fn find_song<'a>(
    songs: &'a [SongEntry],
    groups: &[qrs_library::Group],
    r: &Riff,
) -> Option<(&'a SongEntry, usize)> {
    let canon = r.canonical();
    let g = groups.iter().find(|g| g.canonical == canon).or_else(|| {
        groups.iter().find(|g| {
            let e = &songs[g.preferred];
            r.is_song(&e.artist, &e.display_title())
        })
    })?;
    Some((&songs[g.preferred], g.members.len()))
}

fn check_item(keys: &Keys, songs: &[SongEntry], groups: &[qrs_library::Group], r: &Riff) -> Report {
    let mut rep = Report::default();
    let Some((e, n_files)) = find_song(songs, groups, r) else {
        rep.error(format!("not in the library: {} - {}", r.artist, r.title));
        return rep;
    };
    let file = Path::new(&e.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| e.path.clone());
    rep.info(format!(
        "song: {}{}",
        file,
        if n_files > 1 {
            format!(
                " (and {} other file{} of it)",
                n_files - 1,
                if n_files == 2 { "" } else { "s" }
            )
        } else {
            String::new()
        }
    ));
    if e.needs_keys {
        rep.error("the file needs RS's keys (--keys)");
        return rep;
    }
    let names: Vec<&str> = e.arrangements.iter().map(|p| p.name.as_str()).collect();
    let Some(part) = r.part_of(&e.arrangements).map(|i| &e.arrangements[i]) else {
        rep.error(format!("no part \"{}\" (it has: {})", r.part, names.join(", ")));
        return rep;
    };
    let sk = (!e.song_key.is_empty()).then_some(e.song_key.as_str());
    let arr = match Package::open(Path::new(&e.path), keys, sk).and_then(|mut p| p.load(Some(&part.key), 1.0))
    {
        Ok(a) => a,
        Err(err) => {
            rep.error(format!("couldn't load the {} part: {err}", part.name));
            return rep;
        }
    };
    let opens = arr.open_string_midi();
    let strings = arr.string_names();
    // a note as string and fret, and its pitch: "G5 (C4)"
    let at = |h: &Hit| {
        format!(
            "{}{} ({})",
            string_name(&strings, h.string),
            h.fret,
            opens
                .get(h.string as usize)
                .map(|o| name(o + h.fret))
                .unwrap_or_default()
        )
    };
    rep.info(format!(
        "part: {} ({}, {} strings{})",
        part.name,
        arr.tuning_name(),
        opens.len(),
        if arr.capo != 0 {
            format!(", capo {}", arr.capo)
        } else {
            String::new()
        }
    ));
    let all = hits(&arr);
    let inside: Vec<Hit> = all
        .iter()
        .copied()
        .filter(|h| h.t >= r.start - AT_START && h.t < r.end - AT_END)
        .collect();
    if inside.is_empty() {
        rep.error("no notes between start and end");
        return rep;
    }
    let mut events: Vec<f64> = inside.iter().map(|h| h.t).collect();
    events.dedup_by(|a, b| (*a - *b).abs() < 0.005);
    let chords = inside
        .iter()
        .filter(|h| h.chord)
        .map(|h| (h.t * 1000.0).round() as i64)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let (first, last) = (inside[0], *inside.last().unwrap_or(&inside[0]));
    rep.info(format!(
        "stretch: {:.1} s, {} notes ({} single, {chords} chord{}), first at {:.3} {}, last at {:.3} {}",
        r.end - r.start,
        events.len(),
        inside.iter().filter(|h| !h.chord).count(),
        if chords == 1 { "" } else { "s" },
        first.t,
        at(&first),
        last.t,
        at(&last)
    ));
    // the stretch's edges against the notes
    if let Some(before) = all.iter().rev().find(|h| h.t < r.start - AT_START)
        && r.start - before.t < 0.25
    {
        rep.warn(format!(
            "start is {:.3} s after a note at {:.3} {}: is that the first note?",
            r.start - before.t,
            before.t,
            at(before)
        ));
    }
    if first.t - r.start > 1.5 {
        rep.warn(format!(
            "start is {:.2} s before the first note: the loop waits that long each time",
            first.t - r.start
        ));
    }
    if let Some(after) = all.iter().find(|h| h.t >= r.end - AT_END)
        && after.t - r.end < 0.1
    {
        rep.warn(if after.t - r.end < 0.005 {
            format!(
                "end is on the next note, at {:.3} {}: its start may be heard as the loop goes round",
                after.t,
                at(after)
            )
        } else {
            format!(
                "end is just {:.3} s before the next note, at {:.3} {}",
                after.t - r.end,
                after.t,
                at(after)
            )
        });
    }
    if let Some(h) = inside.iter().rev().find(|h| h.t + h.sus > r.end + 0.05) {
        rep.warn(format!(
            "end cuts the note at {:.3} {}: it rings to {:.3}",
            h.t,
            at(h),
            h.t + h.sus
        ));
    }
    match r.kind {
        Kind::Ear => check_ear(&mut rep, r, &all, &at),
        Kind::Learn => {
            // (v3: whole parts, up to the whole song)
            if r.end - r.start > 300.0 {
                rep.warn("over five minutes long: is the end right?");
            }
        }
        Kind::Scale => check_scale(&mut rep, r, &inside, &opens, &strings),
        Kind::Theory => {
            let other = |name: &str| -> Result<Vec<Hit>, String> {
                let want = Riff {
                    part: name.into(),
                    ..r.clone()
                };
                let i = want
                    .part_of(&e.arrangements)
                    .ok_or_else(|| format!("no part \"{name}\" (it has: {})", names.join(", ")))?;
                let key = &e.arrangements[i].key;
                Package::open(Path::new(&e.path), keys, sk)
                    .and_then(|mut p| p.load(Some(key), 1.0))
                    .map(|a| hits(&a))
                    .map_err(|err| format!("couldn't load the {name} part: {err}"))
            };
            check_theory(&mut rep, r, &all, &opens, &strings, &other);
        }
    }
    check_callouts(&mut rep, r);
    rep
}

/// Callouts: inside the stretch, with text, far enough apart to each be read.
fn check_callouts(rep: &mut Report, r: &Riff) {
    if r.callouts.is_empty() {
        return;
    }
    let mut times: Vec<f64> = Vec::new();
    for c in &r.callouts {
        if c.at < r.start - 0.05 || c.at > r.end + 0.05 {
            rep.error(format!(
                "the callout at {:.2} (\"{}\") is outside the stretch {:.2}-{:.2}",
                c.at,
                short(&c.text),
                r.start,
                r.end
            ));
        }
        if c.text.trim().is_empty() {
            rep.error(format!("the callout at {:.2} has no text", c.at));
        }
        times.push(c.at);
    }
    times.sort_by(f64::total_cmp);
    for w in times.windows(2) {
        // (the card shows the one whose moment is nearest, so each gets about half the gap either side)
        if w[1] - w[0] < CALLOUT_GAP {
            rep.warn(format!(
                "callouts at {:.2} and {:.2} are {:.1} s apart: each is on the card too briefly to read",
                w[0],
                w[1],
                w[1] - w[0]
            ));
        }
    }
    rep.info(format!(
        "{} callout{}",
        r.callouts.len(),
        if r.callouts.len() == 1 { "" } else { "s" }
    ));
}

/// Callouts closer than this (seconds) are each on the card too briefly to read while playing.
const CALLOUT_GAP: f64 = 1.0;

/// The start of a text, for a line of the report.
fn short(s: &str) -> String {
    let mut out: String = s.chars().take(40).collect();
    if s.chars().count() > 40 {
        out.push('…');
    }
    out
}

/// A theory item's pages on the chart: each scale's shape going up on its tuning with its root among its notes, a
/// mark named as a note on that note, a find's targets, a jam's stretch with notes in its part (`other` loads a part
/// by name, for a jam over another part).
fn check_theory(
    rep: &mut Report,
    r: &Riff,
    all: &[Hit],
    opens: &[i32],
    strings: &[&str],
    other: &dyn Fn(&str) -> Result<Vec<Hit>, String>,
) {
    let place = |s: i32, f: i32| {
        format!(
            "{}{} ({})",
            string_name(strings, s),
            f,
            opens
                .get(s as usize)
                .map(|o| name(o + f))
                .unwrap_or_else(|| "no such string".into())
        )
    };
    for (i, pg) in r.pages.iter().enumerate() {
        let sh = &pg.show;
        let mut what: Vec<String> = Vec::new();
        if let Some(sc) = &sh.scale {
            what.push(format!("{} ({} notes)", sc.name, sc.shape.len()));
        }
        if sh.intervals {
            what.push("intervals".into());
        }
        if sh.emphasis {
            what.push("emphasis".into());
        }
        if !sh.marks.is_empty() {
            what.push(format!("{} marks", sh.marks.len()));
        }
        if !sh.chords.is_empty() {
            let names: Vec<&str> = sh.chords.iter().map(|c| c.name.as_str()).collect();
            what.push(format!("chords {}", names.join(", ")));
        }
        rep.info(format!(
            "page {} \"{}\" [{}]: {}",
            i + 1,
            pg.title,
            format!("{:?}", pg.act).to_lowercase(),
            if what.is_empty() {
                "text only".to_string()
            } else {
                what.join("; ")
            }
        ));
        let at = format!("page {}", i + 1);
        if let Some(sc) = &sh.scale {
            if let Some(k) = sc.out_of_order(opens) {
                let (s, f) = sc.shape[k];
                rep.error(format!(
                    "{at}: the scale's shape isn't in ascending pitch order on this tuning: note {} ([{s}, {f}]) isn't higher than the one before",
                    k + 1
                ));
            }
            let pcs = sc.pitch_classes(opens);
            match sc.root_pc() {
                Some(pc) if pcs & (1 << pc) != 0 => {}
                Some(pc) => rep.error(format!(
                    "{at}: the root {} isn't one of the shape's notes",
                    pc_name(pc)
                )),
                None => rep.error(format!("{at}: the root \"{}\" isn't a note name", sc.root)),
            }
        }
        for (s, f, label) in &sh.marks {
            let Some(m) = opens.get(*s as usize).map(|o| o + f) else {
                rep.error(format!(
                    "{at}: the mark [{s}, {f}] is on a string this part hasn't got"
                ));
                continue;
            };
            // a mark named as a note ("Bb") should be on that note ("IV Eb" and "3 of Bb" aren't names)
            if let Some(pc) = lessons::pitch_class(label)
                && pc != m.rem_euclid(12)
            {
                rep.warn(format!("{at}: the mark \"{label}\" is on {}", place(*s, *f)));
            }
        }
        if pg.act == lessons::Activity::Find {
            let names: Vec<String> = pg.targets.iter().map(|&(s, f)| place(s, f)).collect();
            rep.info(format!("{at}: find {}", names.join(", ")));
            if pg.targets.iter().any(|&(s, _)| s as usize >= opens.len()) {
                rep.error(format!("{at}: a target is on a string this part hasn't got"));
            }
        }
        if pg.act == lessons::Activity::Jam {
            let (a, b) = match (pg.start, pg.end) {
                (Some(a), Some(b)) => (a, b),
                _ => (r.start, r.end),
            };
            let part = pg.part.clone().unwrap_or_else(|| r.part.clone());
            let notes = if part.eq_ignore_ascii_case(&r.part) {
                Ok(all.to_vec())
            } else {
                other(&part)
            };
            match notes {
                Err(e) => rep.error(format!("{at}: jam: {e}")),
                Ok(hs) => {
                    let n = hs
                        .iter()
                        .filter(|h| h.t >= a - AT_START && h.t < b - AT_END)
                        .count();
                    if n == 0 {
                        rep.error(format!(
                            "{at}: jam: no notes in the {part} part between {a:.2} and {b:.2}"
                        ));
                    } else {
                        rep.info(format!(
                            "{at}: jam over {a:.2}-{b:.2} ({:.1} s), {n} notes in the {part} part",
                            b - a
                        ));
                    }
                }
            }
        }
    }
}

/// By ear: the phrase found one note at a time (as the coach takes it: single notes from the start, not muted).
fn check_ear(rep: &mut Report, r: &Riff, all: &[Hit], at: &dyn Fn(&Hit) -> String) {
    let mut phrase: Vec<&Hit> = all
        .iter()
        .filter(|h| !h.chord && !h.muted && h.t >= r.start - AT_START && h.t < r.end - AT_END)
        .collect();
    let every = phrase.len();
    if r.phrase > 0 {
        phrase.truncate(r.phrase);
    }
    if phrase.is_empty() {
        rep.error("no single notes to find by ear in the stretch (chords only?): make it a learn item");
        return;
    }
    if r.phrase > phrase.len() {
        rep.warn(format!(
            "phrase is {} but the stretch has only {} single notes",
            r.phrase,
            phrase.len()
        ));
    }
    if r.phrase == 0 && every > 16 {
        rep.warn(format!(
            "{every} notes to find one by one: set phrase to 16 or fewer"
        ));
    }
    let mut strings: Vec<i32> = phrase.iter().map(|h| h.string).collect();
    strings.sort();
    strings.dedup();
    rep.info(format!(
        "phrase ({} of {every} single notes, on {} string{}): {}",
        phrase.len(),
        strings.len(),
        if strings.len() == 1 { "" } else { "s" },
        phrase.iter().map(|h| at(h)).collect::<Vec<_>>().join(", ")
    ));
    if r.string_hint && strings.len() > 1 {
        // (the coach names them all up front, and each note's hints start with its string)
        rep.info(format!(
            "string_hint: the coach names all {} strings",
            strings.len()
        ));
    }
}

/// A scale: the shape on the chart's tuning (going up?), the root among its notes, and the stretch inside it.
fn check_scale(rep: &mut Report, r: &Riff, inside: &[Hit], opens: &[i32], strings: &[&str]) {
    let Some(sc) = &r.scale else {
        rep.error("no scale");
        return;
    };
    let midis = sc.midis(opens);
    let shape: Vec<String> = sc
        .shape
        .iter()
        .zip(&midis)
        .map(|(&(s, f), m)| {
            format!(
                "{}{} ({})",
                string_name(strings, s),
                f,
                m.map(name).unwrap_or_else(|| "no such string".into())
            )
        })
        .collect();
    rep.info(format!(
        "scale: {} ({} notes): {}",
        sc.name,
        sc.shape.len(),
        shape.join(", ")
    ));
    if let Some(i) = sc.out_of_order(opens) {
        let (s, f) = sc.shape[i];
        rep.error(format!(
            "the shape isn't in ascending pitch order on this tuning: note {} ([{s}, {f}]) isn't higher than the one before",
            i + 1
        ));
    }
    let pcs = sc.pitch_classes(opens);
    let names: Vec<&str> = (0..12).filter(|pc| pcs & (1 << pc) != 0).map(pc_name).collect();
    rep.info(format!("its notes: {}", names.join(" ")));
    match sc.root_pc() {
        Some(pc) if pcs & (1 << pc) != 0 => {}
        Some(pc) => rep.error(format!("the root {} isn't one of the shape's notes", pc_name(pc))),
        None => rep.error(format!("the root \"{}\" isn't a note name", sc.root)),
    }
    let pitched: Vec<i32> = inside
        .iter()
        .filter(|h| !h.muted)
        .filter_map(|h| opens.get(h.string as usize).map(|o| o + h.fret))
        .collect();
    if pitched.is_empty() {
        return;
    }
    let inn = pitched.iter().filter(|&&m| in_scale(pcs, m)).count();
    let share = inn as f64 / pitched.len() as f64;
    let mut out: Vec<(i32, usize)> = Vec::new();
    for &m in pitched.iter().filter(|&&m| !in_scale(pcs, m)) {
        match out.iter_mut().find(|o| o.0 == m.rem_euclid(12)) {
            Some(o) => o.1 += 1,
            None => out.push((m.rem_euclid(12), 1)),
        }
    }
    let outside = if out.is_empty() {
        String::new()
    } else {
        format!(
            "; outside: {}",
            out.iter()
                .map(|(pc, k)| format!("{} x{k}", pc_name(*pc)))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    rep.info(format!(
        "the stretch in the scale: {}% of {} notes{outside}",
        (share * 100.0).round() as i64,
        pitched.len()
    ));
    if share < 0.6 {
        rep.warn("under 60% of the stretch is in the scale: the right scale, or the right stretch?");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_library::lessons::Scale;

    const STANDARD: [i32; 6] = [40, 45, 50, 55, 59, 64];
    const NAMES: [&str; 6] = ["E", "A", "D", "G", "B", "E"];

    fn hit(t: f64, string: i32, fret: i32) -> Hit {
        Hit {
            t,
            string,
            fret,
            sus: 0.2,
            muted: false,
            chord: false,
        }
    }

    fn scale_item(root: &str, shape: Vec<(i32, i32)>) -> Riff {
        Riff {
            id: "s".into(),
            kind: Kind::Scale,
            scale: Some(Scale {
                name: "test".into(),
                root: root.into(),
                shape,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_scale_is_checked_on_the_charts_tuning() {
        // A minor pentatonic, box 1; the stretch A C D E plus one Bb
        let box1 = vec![
            (0, 5),
            (0, 8),
            (1, 5),
            (1, 7),
            (2, 5),
            (2, 7),
            (3, 5),
            (3, 7),
            (4, 5),
            (4, 8),
            (5, 5),
            (5, 8),
        ];
        let notes = [
            hit(1.0, 1, 0),
            hit(1.5, 1, 3),
            hit(2.0, 2, 0),
            hit(2.5, 2, 2),
            hit(3.0, 1, 1),
        ];
        let mut rep = Report::default();
        check_scale(
            &mut rep,
            &scale_item("A", box1.clone()),
            &notes,
            &STANDARD,
            &NAMES,
        );
        assert_eq!((rep.errors, rep.warnings), (0, 0), "{:?}", rep.lines);
        let text = rep.lines.join(
            "
",
        );
        assert!(text.contains("its notes: C D E G A"), "{text}");
        assert!(text.contains("80% of 5 notes; outside: A# x1"), "{text}");
        assert!(text.contains("e8 (C5)"), "the high string is e: {text}");
        // a root that isn't one of the shape's notes, and a shape that goes down
        let mut rep = Report::default();
        check_scale(&mut rep, &scale_item("F", box1), &notes, &STANDARD, &NAMES);
        assert_eq!(rep.errors, 1, "{:?}", rep.lines);
        let mut rep = Report::default();
        check_scale(
            &mut rep,
            &scale_item("C", vec![(1, 5), (0, 8)]),
            &notes,
            &STANDARD,
            &NAMES,
        );
        assert_eq!(rep.errors, 1, "{:?}", rep.lines);
        // on Drop D the same frets on the low string are two lower: still in order here, other notes
        let mut drop_d = STANDARD;
        drop_d[0] = 38;
        let mut rep = Report::default();
        check_scale(
            &mut rep,
            &scale_item("A", vec![(0, 5), (0, 8), (1, 5)]),
            &notes,
            &drop_d,
            &NAMES,
        );
        assert!(rep.lines.join(" ").contains("E5 (G2)"), "{:?}", rep.lines);
    }

    #[test]
    fn theory_pages_are_checked_on_the_charts_tuning() {
        use qrs_library::lessons::{Activity, Page, Show};
        let bb = Scale {
            name: "Bb minor pentatonic".into(),
            root: "Bb".into(),
            shape: vec![(0, 6), (0, 9), (1, 6), (1, 8)],
        };
        let page = |title: &str, show: Show, act: Activity| Page {
            title: title.into(),
            show,
            act,
            ..Default::default()
        };
        let mut jam = page(
            "Jam",
            Show {
                scale: Some(bb.clone()),
                ..Default::default()
            },
            Activity::Jam,
        );
        jam.start = Some(10.0);
        jam.end = Some(20.0);
        jam.part = Some("Rhythm".into());
        let r = Riff {
            id: "t".into(),
            kind: Kind::Theory,
            part: "Lead".into(),
            start: 1.0,
            end: 5.0,
            pages: vec![
                page(
                    "The key",
                    Show {
                        marks: vec![(0, 6, "Bb".into()), (1, 1, "Bb".into()), (1, 6, "IV Eb".into())],
                        ..Default::default()
                    },
                    Activity::Read,
                ),
                page(
                    "The scale",
                    Show {
                        scale: Some(bb.clone()),
                        intervals: true,
                        ..Default::default()
                    },
                    Activity::Run,
                ),
                jam,
            ],
            ..Default::default()
        };
        let all = [hit(1.5, 0, 6)];
        let rhythm = |name: &str| -> Result<Vec<Hit>, String> {
            assert_eq!(name, "Rhythm");
            Ok(vec![hit(12.0, 1, 1)])
        };
        let mut rep = Report::default();
        check_theory(&mut rep, &r, &all, &STANDARD, &NAMES, &rhythm);
        assert_eq!((rep.errors, rep.warnings), (0, 0), "{:?}", rep.lines);
        let text = rep.lines.join(" | ");
        assert!(
            text.contains("page 3: jam over 10.00-20.00 (10.0 s), 1 notes in the Rhythm part"),
            "{text}"
        );
        // a jam over a stretch with nothing in it, a mark named as another note, a shape going down
        let nothing = |_: &str| -> Result<Vec<Hit>, String> { Ok(Vec::new()) };
        let mut bad = r.clone();
        bad.pages[0].show.marks.push((0, 5, "Bb".into()));
        bad.pages[1].show.scale.as_mut().unwrap().shape = vec![(1, 1), (0, 6)]; // (Bb, then Bb again)
        let mut rep = Report::default();
        check_theory(&mut rep, &bad, &all, &STANDARD, &NAMES, &nothing);
        assert_eq!((rep.errors, rep.warnings), (2, 1), "{:?}", rep.lines);
    }

    #[test]
    fn callouts_inside_the_stretch() {
        let co = |at: f64, text: &str| qrs_library::lessons::Callout {
            at,
            text: text.into(),
        };
        let r = Riff {
            id: "l".into(),
            start: 10.0,
            end: 40.0,
            callouts: vec![co(12.0, "a"), co(30.0, "b"), co(30.6, "c"), co(32.0, "d")],
            ..Default::default()
        };
        let mut rep = Report::default();
        check_callouts(&mut rep, &r);
        assert_eq!((rep.errors, rep.warnings), (0, 1), "{:?}", rep.lines);
        let mut out = r.clone();
        out.callouts.push(co(45.0, "after the end"));
        out.callouts.push(co(20.0, " "));
        let mut rep = Report::default();
        check_callouts(&mut rep, &out);
        assert_eq!(rep.errors, 2, "{:?}", rep.lines);
    }

    #[test]
    fn the_phrase_by_ear_is_the_stretchs_single_notes() {
        let r = Riff {
            id: "e".into(),
            start: 1.0,
            end: 3.0,
            phrase: 2,
            string_hint: true,
            ..Default::default()
        };
        let all = [
            hit(0.9, 3, 0),
            hit(0.99, 3, 0),
            hit(1.5, 3, 3),
            hit(2.0, 2, 5),
            hit(2.995, 3, 5),
        ];
        let at = |h: &Hit| format!("{}{}", h.string, h.fret);
        let mut rep = Report::default();
        check_ear(&mut rep, &r, &all, &at);
        assert_eq!((rep.errors, rep.warnings), (0, 0), "{:?}", rep.lines);
        assert!(
            rep.lines[0].contains("(2 of 3 single notes, on 1 string): 30, 33"),
            "{:?}",
            rep.lines
        );
        // all of them: two strings, both named
        let mut rep = Report::default();
        check_ear(
            &mut rep,
            &Riff {
                phrase: 0,
                ..r.clone()
            },
            &all,
            &at,
        );
        assert_eq!(rep.warnings, 0, "{:?}", rep.lines);
        assert!(rep.lines[1].contains("names all 2 strings"), "{:?}", rep.lines);
        // nothing to find
        let mut rep = Report::default();
        check_ear(
            &mut rep,
            &Riff {
                start: 5.0,
                end: 6.0,
                ..r
            },
            &all,
            &at,
        );
        assert_eq!(rep.errors, 1);
    }
}
