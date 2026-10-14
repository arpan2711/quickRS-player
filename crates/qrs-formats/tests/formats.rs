//! Ports of the Python suite's test_sng.py, test_xml.py, test_psarc.py and test_loader_cli.py (package parts),
//! plus feedpak tests on the spec's example packs (MIT, tests/fixtures/feedpak).

use std::io::Write;
use std::path::{Path, PathBuf};

use qrs_formats::keys::Keys;
use qrs_formats::model::{Arrangement, Techs};
use qrs_formats::package::{Format, Package, load_arrangement};
use qrs_formats::psarc::{Platform, Psarc, to_mac_name, write_psarc};
use qrs_formats::sng::{self, ArrMeta, decrypt_sng, load_sng, parse_sng, sng_to_arrangement};
use qrs_formats::testutil::*;
use qrs_formats::xml::parse_xml;

fn meta(name: &str) -> ArrMeta<'_> {
    ArrMeta {
        name,
        is_bass: false,
        title: "Demo",
        artist: "",
        album: "",
    }
}

fn demo_arrangement() -> Arrangement {
    sng_to_arrangement(&parse_sng(&demo_sng_plain()).unwrap(), &meta("Lead"), 1.0).unwrap()
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// --- SNG -------------------------------------------------------------------------------------------------
#[test]
fn sng_decrypt_roundtrip() {
    let plain = demo_sng_plain();
    assert_eq!(decrypt_sng(&pack_test_sng(&plain), &TEST_SNG_KEY).unwrap(), plain);
}

#[test]
fn sng_wrong_key_is_reported() {
    let err = decrypt_sng(&pack_test_sng(&demo_sng_plain()), &[0u8; 32]).unwrap_err();
    assert!(
        err.to_string().contains("wrong SNG key") || err.to_string().contains("size mismatch"),
        "{err}"
    );
}

#[test]
fn sng_missing_key() {
    let err = load_sng(&pack_test_sng(&demo_sng_plain()), &Keys::default(), Platform::Pc).unwrap_err();
    assert!(err.is_key_missing(), "{err}");
}

#[test]
fn plain_sng_needs_no_key() {
    let s = load_sng(&demo_sng_plain(), &Keys::default(), Platform::Pc).unwrap();
    assert!((s.metadata.song_length - 6.5).abs() < 1e-6);
}

#[test]
fn sng_structure() {
    let s = parse_sng(&demo_sng_plain()).unwrap();
    assert_eq!((s.beats.len(), s.phrases.len(), s.levels.len()), (14, 3, 2));
    assert_eq!(
        s.chord_templates
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["E5", "G"]
    );
    assert_eq!(s.metadata.tuning, vec![-2, 0, 0, 0, 0, 0]);
    assert_eq!(s.metadata.capo, 0); // -1 in the file means no capo
    assert_eq!(
        s.sections.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["riff", "noguitar"]
    );
}

#[test]
fn sng_vocal_symbol_sections_are_skipped() {
    let mut spec = demo_spec();
    spec.vocals = 3;
    let s = parse_sng(&build_sng_plain(&spec)).unwrap();
    assert_eq!(s.vocals.len(), 3);
    assert_eq!(s.phrase_iterations.len(), 3); // parsing stayed aligned after the symbol sections
}

#[test]
fn sng_truncated_data_raises() {
    let p = demo_sng_plain();
    assert!(parse_sng(&p[..p.len() - 20]).is_err());
}

#[test]
fn sng_full_difficulty_selection() {
    let arr = demo_arrangement();
    let times: Vec<f64> = arr.events().iter().map(|e| e.time()).collect();
    let want = [1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 5.5];
    assert_eq!(times.len(), want.len());
    assert!(times.iter().zip(want).all(|(a, b)| approx(*a, b)), "{times:?}");
    assert!(arr.notes.iter().all(|n| n.fret != 99));
    let last = arr.notes.last().unwrap();
    assert!(last.fret == 12 && last.has(Techs::HARMONIC));
}

#[test]
fn sng_mastery_zero_uses_level_zero() {
    let s = parse_sng(&demo_sng_plain()).unwrap();
    let arr = sng_to_arrangement(&s, &meta("Lead"), 0.0).unwrap();
    // level 0 of the riff phrase holds only the first note; END keeps its harmonic
    let frets: Vec<i32> = arr.notes.iter().map(|n| n.fret).collect();
    assert_eq!(frets, vec![3, 12]);
    assert!(arr.has_levels);
}

#[test]
fn sng_techniques_and_chords() {
    let arr = demo_arrangement();
    let at = |t: f64| arr.notes.iter().find(|n| approx(n.time, t)).unwrap();
    assert!(at(1.25).has(Techs::HAMMER_ON));
    assert!(at(3.5).has(Techs::PULL_OFF));
    let bend = at(2.0);
    assert!(bend.has(Techs::BEND) && approx(bend.bend, 1.0) && (bend.sustain - 0.4).abs() < 1e-6);
    assert_eq!(bend.bend_curve.len(), 2);
    assert!(at(2.5).slide_to == 7 && at(2.5).has(Techs::SLIDE));
    assert!(at(3.0).has(Techs::FRET_HAND_MUTE)); // single-note mute uses bit 0x20000
    let (e5, g) = (&arr.chords[0], &arr.chords[1]);
    assert_eq!(e5.name, "E5");
    assert_eq!(
        e5.notes.iter().map(|n| (n.string, n.fret)).collect::<Vec<_>>(),
        vec![(0, 0), (1, 2), (2, 2)]
    );
    assert!(e5.techniques.has(Techs::PALM_MUTE) && e5.notes.iter().all(|n| n.has(Techs::PALM_MUTE)));
    assert!(e5.notes[2].has(Techs::VIBRATO) && !e5.notes[0].has(Techs::VIBRATO));
    assert!(g.name == "G" && g.notes.len() == 6);
    assert!(g.notes.iter().all(|n| n.has(Techs::FRET_HAND_MUTE))); // chord mute uses bit 0x8
    assert_eq!(g.frets, vec![3, 2, 0, 0, 0, 3]);
}

#[test]
fn sng_metadata_and_beats() {
    let arr = demo_arrangement();
    assert_eq!(arr.tuning_name(), "Drop D");
    assert!(approx(arr.song_length, 6.5));
    assert_eq!(
        arr.beats[..5].iter().map(|b| b.measure).collect::<Vec<_>>(),
        vec![1, -1, -1, -1, 2]
    );
}

// --- XML -------------------------------------------------------------------------------------------------
type Sig = (
    Vec<(String, i32, i32, Vec<&'static str>, i32, String, String)>,
    Vec<(
        String,
        String,
        Vec<&'static str>,
        Vec<(i32, i32, Vec<&'static str>)>,
    )>,
);

fn signature(arr: &Arrangement) -> Sig {
    let notes = arr
        .notes
        .iter()
        .map(|n| {
            (
                format!("{:.3}", n.time),
                n.string,
                n.fret,
                n.techniques.names(),
                n.slide_to,
                format!("{:.2}", n.bend),
                format!("{:.3}", n.sustain),
            )
        })
        .collect();
    let chords = arr
        .chords
        .iter()
        .map(|c| {
            (
                format!("{:.3}", c.time),
                c.name.clone(),
                c.techniques.names(),
                c.notes
                    .iter()
                    .map(|n| (n.string, n.fret, n.techniques.names()))
                    .collect(),
            )
        })
        .collect();
    (notes, chords)
}

#[test]
fn xml_matches_sng_for_the_same_chart() {
    let x = parse_xml(demo_xml().as_bytes(), 1.0).unwrap();
    let s = demo_arrangement();
    assert_eq!(signature(&x), signature(&s));
    assert_eq!(x.beats.len(), s.beats.len());
    for (a, b) in x.beats.iter().zip(&s.beats) {
        assert!(approx(a.time, b.time) && a.measure == b.measure);
    }
}

#[test]
fn xml_metadata() {
    let arr = parse_xml(demo_xml().as_bytes(), 1.0).unwrap();
    assert_eq!(
        (
            arr.title.as_str(),
            arr.artist.as_str(),
            arr.album.as_str(),
            arr.name.as_str()
        ),
        ("Demo Song", "Test Band", "Fixtures", "Lead")
    );
    assert_eq!(arr.tuning, vec![-2, 0, 0, 0, 0, 0]);
    assert_eq!(arr.tuning_name(), "Drop D");
    assert_eq!(
        arr.sections.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["riff", "noguitar"]
    );
    assert!(approx(arr.sections[0].end, 5.0));
}

#[test]
fn xml_chord_without_chord_notes_uses_template() {
    let g = parse_xml(demo_xml().as_bytes(), 1.0).unwrap().chords[1].clone();
    assert_eq!(
        g.notes.iter().map(|n| n.fret).collect::<Vec<_>>(),
        vec![3, 2, 0, 0, 0, 3]
    );
    assert!(g.notes.iter().all(|n| n.has(Techs::FRET_HAND_MUTE)));
}

#[test]
fn xml_rejects_vocals_and_garbage() {
    assert!(
        parse_xml(b"<vocals count='0'/>", 1.0)
            .unwrap_err()
            .to_string()
            .contains("vocals")
    );
    assert!(
        parse_xml(b"<song>", 1.0)
            .unwrap_err()
            .to_string()
            .contains("invalid XML")
    );
    assert!(
        parse_xml(b"<html/>", 1.0)
            .unwrap_err()
            .to_string()
            .contains("not a Rocksmith")
    );
}

// --- PSARC -----------------------------------------------------------------------------------------------
fn files() -> Vec<(String, Vec<u8>)> {
    let noise: Vec<u8> = (0..3000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 7) as u8)
        .collect();
    vec![
        ("songs/bin/generic/a_lead.sng".into(), b"sng data ".repeat(100)),
        (
            "manifests/songs_dlc_a/a_lead.json".into(),
            b"{\"Entries\": {}}".to_vec(),
        ),
        ("audio/windows/1.wem".into(), noise),
    ]
}

#[test]
fn psarc_raw_full_blocks_use_zero_size_marker() {
    let raw: Vec<u8> = (0..(65536 * 2 + 10) as u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 11) as u8)
        .collect();
    let mut a = Psarc::from_bytes(
        write_psarc(&[("r.wem".into(), raw.clone())], None, 65536, true),
        None,
    )
    .unwrap();
    assert_eq!(a.read("r.wem").unwrap(), raw);
}

#[test]
fn psarc_raw_block_that_looks_like_zlib_falls_back_to_raw() {
    let mut data = vec![0x78, 0xda];
    data.extend((0..500u32).map(|i| (i.wrapping_mul(2654435761) >> 9) as u8));
    let mut a = Psarc::from_bytes(
        write_psarc(&[("odd.wem".into(), data.clone())], None, 65536, false),
        None,
    )
    .unwrap();
    assert_eq!(a.read("odd.wem").unwrap(), data);
}

#[test]
fn psarc_find_is_case_insensitive_glob() {
    let a = Psarc::from_bytes(write_psarc(&files(), None, 65536, true), None).unwrap();
    assert_eq!(
        a.find("SONGS/bin/*.sng")
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["songs/bin/generic/a_lead.sng"]
    );
    assert_eq!(a.get("AUDIO/WINDOWS/1.WEM").unwrap().name, "audio/windows/1.wem");
    assert!(a.get("nope").is_err());
}

#[test]
fn psarc_extract_and_path_traversal_is_contained() {
    let dir = tempfile::tempdir().unwrap();
    let mut fs = files();
    fs.push(("../../evil.txt".into(), b"x".to_vec()));
    fs.push(("/abs/thing.txt".into(), b"y".to_vec()));
    let mut a = Psarc::from_bytes(write_psarc(&fs, None, 65536, true), None).unwrap();
    let written = a.extract(dir.path(), None).unwrap();
    assert_eq!(written.len(), fs.len());
    for p in &written {
        assert!(p.starts_with(dir.path()));
    }
    assert_eq!(std::fs::read(dir.path().join("evil.txt")).unwrap(), b"x");
    let w = a.extract(&dir.path().join("m"), Some("manifests/*")).unwrap();
    assert_eq!(w.len(), 1);
}

// --- packages --------------------------------------------------------------------------------------------
fn write_tmp(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, data).unwrap();
    p
}

#[test]
fn package_discovers_instrument_arrangements() {
    let dir = tempfile::tempdir().unwrap();
    let song = write_tmp(dir.path(), "demo_p.psarc", &demo_psarc(Some(&TEST_PSARC_KEY)));
    let mut pkg = Package::open(&song, &test_keys(), None).unwrap();
    assert_eq!(pkg.format(), Format::Psarc);
    let got: Vec<(&str, &str, bool)> = pkg
        .arrangements
        .iter()
        .map(|a| (a.key.as_str(), a.name.as_str(), a.is_bass))
        .collect();
    assert_eq!(got, vec![("lead", "Lead", false), ("bass", "Bass", true)]);
    assert_eq!(
        pkg.arrangements[0].chart.describe(),
        "songs/bin/generic/demo_lead.sng"
    );
    assert_eq!(pkg.audio_files()[0].0, "audio/windows/111.wem");
    assert_eq!(pkg.find(Some("2")).unwrap().key, "bass");
    assert_eq!(pkg.find(Some("BA")).unwrap().key, "bass");
    assert_eq!(pkg.find(None).unwrap().key, "lead");
    assert!(
        pkg.find(Some("drums"))
            .unwrap_err()
            .to_string()
            .contains("available: lead, bass")
    );
}

#[test]
fn load_from_package() {
    let dir = tempfile::tempdir().unwrap();
    let song = write_tmp(dir.path(), "demo_p.psarc", &demo_psarc(Some(&TEST_PSARC_KEY)));
    let arr = load_arrangement(&song, Some("bass"), &test_keys()).unwrap();
    assert_eq!(
        (
            arr.title.as_str(),
            arr.artist.as_str(),
            arr.name.as_str(),
            arr.is_bass
        ),
        ("Demo Song", "Test Band", "Bass", true)
    );
    // The fixture's "bass" reuses the guitar chart, whose G chord uses all six strings.
    assert_eq!(arr.string_names(), vec!["D", "A", "D", "G", "B", "E"]);
    assert_eq!(arr.events().len(), 9);
}

#[test]
fn load_loose_files() {
    let dir = tempfile::tempdir().unwrap();
    let sngp = write_tmp(dir.path(), "demo_lead.sng", &pack_test_sng(&demo_sng_plain()));
    let xmlp = write_tmp(dir.path(), "demo_lead.xml", demo_xml().as_bytes());
    assert_eq!(load_arrangement(&sngp, None, &test_keys()).unwrap().name, "Lead");
    assert_eq!(
        load_arrangement(&xmlp, None, &Keys::default()).unwrap().title,
        "Demo Song"
    );
    assert!(
        load_arrangement(&sngp, None, &Keys::default())
            .unwrap_err()
            .is_key_missing()
    );
    assert!(
        load_arrangement(&dir.path().join("x.mp3"), None, &Keys::default())
            .unwrap_err()
            .to_string()
            .contains("unsupported file type")
    );
}

#[test]
fn json_export_shape() {
    let dir = tempfile::tempdir().unwrap();
    let song = write_tmp(dir.path(), "demo_p.psarc", &demo_psarc(Some(&TEST_PSARC_KEY)));
    let arr = load_arrangement(&song, None, &test_keys()).unwrap();
    let v = arr.to_json();
    assert_eq!(v["arrangement"], "Lead");
    assert_eq!(v["notes"].as_array().unwrap().len(), 7);
    assert_eq!(v["chords"].as_array().unwrap().len(), 2);
    assert_eq!(v["tuning"]["name"], "Drop D");
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "title",
            "artist",
            "album",
            "arrangement",
            "source",
            "is_bass",
            "tuning",
            "capo",
            "song_length",
            "beats",
            "sections",
            "notes",
            "chords"
        ]
    );
}

#[test]
fn mac_package_reads_like_pc() {
    let mac_key: [u8; 32] = core::array::from_fn(|i| 200u8.wrapping_add(i as u8));
    let files: Vec<(String, Vec<u8>)> = demo_psarc_files()
        .into_iter()
        .map(|(n, d)| {
            let d = if n.ends_with(".sng") {
                sng::pack_sng(&demo_sng_plain(), &mac_key, &TEST_IV)
            } else {
                d
            };
            (to_mac_name(&n), d)
        })
        .collect();
    let keys = Keys {
        sng_mac: Some(mac_key),
        ..test_keys()
    };
    let mut pkg = Package::from_psarc_bytes(
        write_psarc(&files, Some(&TEST_PSARC_KEY), 65536, true),
        "demo_m.psarc",
        &keys,
        None,
    )
    .unwrap();
    assert_eq!(pkg.platform(), Platform::Mac);
    assert_eq!(pkg.load(Some("lead"), 1.0).unwrap().events().len(), 9);
    // without the Mac key: a clear message
    let mut pkg2 = Package::from_psarc_bytes(
        write_psarc(&files, Some(&TEST_PSARC_KEY), 65536, true),
        "demo_m.psarc",
        &test_keys(),
        None,
    )
    .unwrap();
    let err = pkg2.load(None, 1.0).unwrap_err().to_string();
    assert!(err.contains("Mac") || err.contains("decryption"), "{err}");
}

#[test]
fn multi_song_pack_filters_by_song_key() {
    let sng = pack_test_sng(&demo_sng_plain());
    let manifest = |song_key: &str, name: &str| -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"Entries": {"x": {"Attributes": {
            "ArrangementName": name, "SongName": song_key, "SongKey": song_key, "SongYear": 1999,
            "SongAsset": format!("urn:application:musicgame-song:{}_{}", song_key.to_lowercase(), name.to_lowercase())
        }}}})).unwrap()
    };
    let files = vec![
        ("songs/bin/generic/aaa_lead.sng".to_string(), sng.clone()),
        ("songs/bin/generic/bbb_lead.sng".to_string(), sng),
        (
            "manifests/songs/aaa_lead.json".to_string(),
            manifest("AAA", "Lead"),
        ),
        (
            "manifests/songs/bbb_lead.json".to_string(),
            manifest("BBB", "Lead"),
        ),
    ];
    let bytes = write_psarc(&files, Some(&TEST_PSARC_KEY), 65536, true);
    let pkg = Package::from_psarc_bytes(bytes.clone(), "songs.psarc", &test_keys(), None).unwrap();
    assert_eq!(pkg.song_keys(), vec!["AAA", "BBB"]);
    let sub = Package::from_psarc_bytes(bytes, "songs.psarc", &test_keys(), Some("bbb")).unwrap();
    assert_eq!(sub.arrangements.len(), 1);
    assert_eq!(sub.title(), "BBB");
    assert_eq!(sub.year(), 1999);
}

// --- feedpak ---------------------------------------------------------------------------------------------
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feedpak")
}

#[test]
fn feedpak_minimal_folder() {
    let mut pkg = Package::open(fixtures().join("minimal.feedpak"), &Keys::default(), None).unwrap();
    assert_eq!(pkg.format(), Format::Feedpak);
    assert_eq!(
        (pkg.title(), pkg.artist()),
        ("Untitled Demo".to_string(), "Example Artist".to_string())
    );
    let arr = pkg.load(None, 1.0).unwrap();
    assert_eq!(arr.notes.len(), 4);
    assert!(arr.notes[2].has(Techs::HAMMER_ON));
    assert!(arr.notes[3].has(Techs::BEND) && approx(arr.notes[3].bend, 1.0));
    assert!(!arr.has_levels);
    assert_eq!(arr.source, "feedpak");
    let fp = pkg.feedpak().unwrap();
    assert_eq!(fp.song_stem().as_deref(), Some("stems/full.ogg"));
}

#[test]
fn feedpak_extended_folder() {
    let mut pkg = Package::open(fixtures().join("extended.feedpak"), &Keys::default(), None).unwrap();
    // keys (notation only) and the drum parts are not fretted charts
    let keys: Vec<&str> = pkg.arrangements.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(keys, vec!["lead", "bass"]);
    assert!(pkg.arrangements[1].is_bass);
    let lead = pkg.load(Some("lead"), 1.0).unwrap();
    assert!(
        !lead.beats.is_empty() && !lead.sections.is_empty(),
        "song_timeline.json gives beats and sections"
    );
    let lyrics = pkg.load_lyrics().unwrap();
    assert_eq!(lyrics.len(), 2);
    assert_eq!(pkg.year(), 2026);
    let fp = pkg.feedpak().unwrap();
    assert!(fp.cover_bytes().is_some());
}

fn zip_dir(src: &Path, dst: &Path, top: Option<&str>) {
    let f = std::fs::File::create(dst).unwrap();
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    let mut stack = vec![src.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(src).unwrap().to_string_lossy().replace('\\', "/");
                let name = match top {
                    Some(t) => format!("{t}/{rel}"),
                    None => rel,
                };
                z.start_file(name, opts).unwrap();
                z.write_all(&std::fs::read(&p).unwrap()).unwrap();
            }
        }
    }
    z.finish().unwrap();
}

#[test]
fn feedpak_zip_and_zip_with_top_folder() {
    let dir = tempfile::tempdir().unwrap();
    for top in [None, Some("song.feedpak")] {
        let z = dir.path().join(format!("x{}.feedpak", top.is_some() as u8));
        zip_dir(&fixtures().join("extended.feedpak"), &z, top);
        let mut pkg = Package::open(&z, &Keys::default(), None).unwrap();
        assert_eq!(pkg.load(Some("bass"), 1.0).unwrap().name, "Bass");
        assert!(pkg.feedpak().unwrap().cover_bytes().is_some());
    }
}

#[test]
fn feedpak_jsonc_ladder_and_unsafe_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("t.feedpak");
    std::fs::create_dir_all(root.join("arrangements")).unwrap();
    std::fs::write(
        root.join("manifest.yaml"),
        "title: T\nartist: A\nduration: 10\narrangements:\n  - id: lead\n    name: Lead\n    file: arrangements/lead.jsonc\n  - id: evil\n    name: Evil\n    file: ../evil.json\nstems:\n  - id: full\n    file: stems/full.ogg\n",
    )
    .unwrap();
    std::fs::write(
        root.join("arrangements/lead.jsonc"),
        r#"{ // a comment
          "notes": [{"t": 1.0, "s": 0, "f": 5}, {"t": 2.0, "s": 0, "f": 7}],
          /* the ladder */
          "phrases": [{"start_time": 0, "end_time": 10, "max_difficulty": 2, "levels": [
             {"difficulty": 0, "notes": [{"t": 1.0, "s": 0, "f": 5}]},
             {"difficulty": 2, "notes": [{"t": 1.0, "s": 0, "f": 5}, {"t": 2.0, "s": 0, "f": 7}]}]}]
        }"#,
    )
    .unwrap();
    let mut pkg = Package::open(&root, &Keys::default(), None).unwrap();
    assert_eq!(pkg.arrangements.len(), 1, "the unsafe part is skipped");
    assert_eq!(pkg.feedpak_ref().unwrap().problems.len(), 1);
    let full = pkg.load(None, 1.0).unwrap();
    assert_eq!(full.notes.len(), 2);
    assert!(full.has_levels);
    let easy = pkg.load(None, 0.2).unwrap(); // round(0.2 * 2) = 0 -> level 0
    assert_eq!(easy.notes.len(), 1);
    let mid = pkg.load(None, 0.75).unwrap(); // round(1.5) = 2 (half to even) -> level 2
    assert_eq!(mid.notes.len(), 2);
    // no stem file on disk: no song stem
    assert!(pkg.feedpak().unwrap().song_stem().is_none());
}
