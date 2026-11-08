//! What the highway is asked to draw across a library (new in quickRS, P46): single notes that start with a chord
//! but aren't in it (drawn on their own, rolled upright with a stem, beside the chord's box), legato and other marks
//! on open strings, accents, bends inside chords, chords flagged as repeats with other notes.
//! Usage: hw_scan <keys.json> <folder or song> ... (prints one line per song with anything to report, then totals)
use qrs_formats::keys::load_keys;
use qrs_formats::model::{Arrangement, Techs};
use qrs_formats::package::Package;
use std::path::{Path, PathBuf};

#[derive(Default, Debug)]
struct Count {
    /// single notes within 10 ms of a chord's time, on a string the chord doesn't use
    beside: usize,
    /// of those, the ones not exactly at the chord's time
    beside_off: usize,
    /// hammer-ons / pull-offs on open strings (single, in chords)
    open_legato: (usize, usize),
    /// any other mark on an open string (palm mute, harmonic, ...)
    open_other: usize,
    accents: usize,
    chord_bends: usize,
    /// chords flagged high density whose notes differ from the chord before
    hd_differ: usize,
}

fn scan(a: &Arrangement) -> Count {
    let mut c = Count::default();
    let mut chords: Vec<_> = a.chords.iter().collect();
    chords.sort_by(|x, y| x.time.total_cmp(&y.time));
    let times: Vec<f64> = chords.iter().map(|ch| ch.time).collect();
    let legato = Techs(Techs::HAMMER_ON.0 | Techs::PULL_OFF.0);
    let other = Techs(
        Techs::PALM_MUTE.0
            | Techs::FRET_HAND_MUTE.0
            | Techs::HARMONIC.0
            | Techs::PINCH_HARMONIC.0
            | Techs::TAP.0,
    );
    for n in &a.notes {
        let i = times.partition_point(|&t| t < n.time - 0.010);
        if let Some(ch) = chords.get(i)
            && (ch.time - n.time).abs() <= 0.010
            && !ch.notes.iter().any(|m| m.string == n.string)
        {
            c.beside += 1;
            if (ch.time - n.time).abs() > 1e-4 {
                c.beside_off += 1;
            }
        }
        if n.fret == 0 && n.techniques.0 & legato.0 != 0 {
            c.open_legato.0 += 1;
        }
        if n.fret == 0 && n.techniques.0 & other.0 != 0 {
            c.open_other += 1;
        }
        if n.techniques.0 & Techs::ACCENT.0 != 0 {
            c.accents += 1;
        }
    }
    for (k, ch) in chords.iter().enumerate() {
        for n in &ch.notes {
            let t = Techs(n.techniques.0 | ch.techniques.0);
            if n.fret == 0 && t.0 & legato.0 != 0 {
                c.open_legato.1 += 1;
            }
            if n.fret == 0 && t.0 & other.0 != 0 {
                c.open_other += 1;
            }
            if n.bend > 0.0 {
                c.chord_bends += 1;
            }
        }
        if ch.techniques.0 & Techs::ACCENT.0 != 0
            || ch.notes.iter().any(|n| n.techniques.0 & Techs::ACCENT.0 != 0)
        {
            c.accents += 1;
        }
        if ch.high_density && k > 0 {
            let key = |x: &qrs_formats::model::Chord| {
                let mut v: Vec<(i32, i32)> = x.notes.iter().map(|n| (n.string, n.fret)).collect();
                v.sort_unstable();
                v
            };
            if key(chords[k - 1]) != key(ch) {
                c.hd_differ += 1;
            }
        }
    }
    c
}

fn songs(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        if let Ok(rd) = std::fs::read_dir(p) {
            let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            v.sort();
            for q in v {
                songs(&q, out);
            }
        }
    } else if p
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("psarc") || e.eq_ignore_ascii_case("feedpak"))
    {
        out.push(p.to_path_buf());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let keys = load_keys(Some(Path::new(&args[0])), &[]).expect("keys");
    let mut files = Vec::new();
    for a in &args[1..] {
        songs(Path::new(a), &mut files);
    }
    let mut tot = Count::default();
    let mut parts = 0;
    for f in &files {
        let Ok(mut pk) = Package::open(f, &keys, None) else {
            continue;
        };
        for info in pk.arrangements.clone() {
            let Ok(a) = pk.load_info(&info, 1.0) else {
                continue;
            };
            parts += 1;
            let c = scan(&a);
            if c.beside
                + c.open_legato.0
                + c.open_legato.1
                + c.open_other
                + c.accents
                + c.chord_bends
                + c.hd_differ
                > 0
            {
                println!("{} - {} [{}]: {:?}", a.artist, a.title, info.name, c);
            }
            tot.beside += c.beside;
            tot.beside_off += c.beside_off;
            tot.open_legato.0 += c.open_legato.0;
            tot.open_legato.1 += c.open_legato.1;
            tot.open_other += c.open_other;
            tot.accents += c.accents;
            tot.chord_bends += c.chord_bends;
            tot.hd_differ += c.hd_differ;
        }
    }
    println!("{} files, {parts} parts: {tot:?}", files.len());
}
