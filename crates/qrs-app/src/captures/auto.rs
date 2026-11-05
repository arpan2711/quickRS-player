//! A song's tone from NAM captures (new in quickRS, P33.2, D82): RS's rig for the tone becomes our board as
//! before (`rs_gear::board_keyed`, which keeps each block's gear key), then every amp, cab, drive and fuzz is
//! swapped for a capture of it from the library:
//! - one set up for the song; for an amp, the amp on the song's record; the capture of that very unit (`Item::rs` has
//!   the gear key; a cab by its body, "GB412CMKII"); for an amp, the real gear behind RS's by the capture's own gear
//!   words (P41.1, D164); else the capture standing in for the same model of ours (`Item::skin`), bass gear for a bass
//!   tone (`pick_why`, which says why: `why_of`);
//! - of those, the one whose gain is nearest the block's (a capture is one setting of the real knobs), ties to the
//!   lowest id, so a tone always gets the same captures;
//! - an amp capture with its cab in it turns the cab off; nothing found leaves our block; an amp capture's post-EQ
//!   gets the song's Bass, Mid and Treble (`song_eq`, P41.1);
//! - only a capture whose file loaded is picked, and the unit of ours it replaces stays, off, after it (P38.2, D153).
//!
//! Delay, reverb, modulation, EQ, gate and compression stay ours: NAM can't capture time-based effects. At most
//! `MAX_CAPTURES` captures a board (each is ~15-20% of a core).

use qrs_audio::board::{Block, Board, Kind, MAX_BLOCKS, rs_gear};
use qrs_formats::rig::RigTone;

use super::{Item, Kind as CKind, Library};

/// Captures on one board at most (amps and pedals; IRs are cheap): two keeps a board under ~45% of a core (the
/// library check: three reached 75%), further drives stay ours.
pub const MAX_CAPTURES: usize = 2;

/// The knob that says how hard a block drives (its gain, else its drive or volume).
pub fn gain_of(b: &Block) -> Option<f32> {
    ["gain", "drive", "dist", "fuzz", "sustain", "volume"]
        .iter()
        .find_map(|k| b.get(k))
}

/// The library kind that stands in for a block of ours, if any.
fn kind_for(b: &Block) -> Option<CKind> {
    let def = b.def();
    if def.file || def.id == "acoustic" {
        return None;
    }
    match def.kind {
        Kind::Amp => Some(CKind::Nam),
        Kind::Drive | Kind::Fuzz => Some(CKind::Pedal),
        Kind::Cab => Some(CKind::Ir),
        _ => None,
    }
}

/// The song a tone is from (P41.1): its title and artist, lower case. A capture set up for the song (`Item::songs`)
/// comes first, then the amp on the record (`RECORDS`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SongHint {
    pub title: String,
    pub artist: String,
}

impl SongHint {
    pub fn new(artist: &str, title: &str) -> SongHint {
        SongHint {
            title: title.trim().to_lowercase(),
            artist: artist.trim().to_lowercase(),
        }
    }
}

/// Why a capture was picked for a block (P41.1, D164): the room's "why this capture".
#[derive(Clone, Debug, PartialEq)]
pub enum Why {
    /// It's set up for this song.
    Song,
    /// The amp on the record (the artist's, `RECORDS`): the words that matched.
    Record(&'static str),
    /// It is the very unit RS's gear stands for (`Item::rs`).
    Unit,
    /// Its gear is the real amp behind RS's (`REAL_GEAR`): the words that matched.
    Gear(&'static str),
    /// It stands in for the same model of ours, nearest in gain.
    Model,
}

impl Why {
    /// A short line ("the amp on the record: hot rod deville (gain 3.0 for 3.0)").
    pub fn line(&self, key: Option<&str>, b: &Block, it: &Item) -> String {
        let rs = key
            .and_then(rs_gear::real_gear)
            .map(|r| r.split(':').next().unwrap_or(r).trim().to_string());
        let gain = match (gain_of(b), it.gain) {
            (Some(o), Some(c)) => format!(" (gain {c:.1} for {o:.1})"),
            _ => String::new(),
        };
        match self {
            Why::Song => "set up for this song".into(),
            Why::Record(w) => format!("the amp on the record: {w}{gain}"),
            Why::Unit => match rs {
                Some(r) => format!("the unit RS's {r} stands for{gain}"),
                None => format!("the unit the song's gear stands for{gain}"),
            },
            Why::Gear(w) => match rs {
                Some(r) => format!("the real gear behind RS's {r} ({w}){gain}"),
                None => format!("the real gear ({w}){gain}"),
            },
            Why::Model => format!("sounds like our {}, nearest in gain{gain}", b.def().name),
        }
    }
}

/// The real amp behind each of RS's (P41.1, D164: "a Dual Rec song gets a Recto capture"), as words a capture's gear
/// or name carries (lower case): the captures whose gear says so come before any of the same model of ours.
pub const REAL_GEAR: &[(&str, &[&str])] = &[
    ("Amp_AT20", &["deluxe reverb"]),
    ("Amp_AT120", &["hot rod"]),
    (
        "Amp_TW26",
        &[
            "twin reverb",
            "super reverb",
            "princeton",
            "vibrolux",
            "pro reverb",
        ],
    ),
    ("Amp_TW22", &["5e3", "tweed deluxe"]),
    ("Amp_TW40", &["bassman"]),
    ("Amp_CS90", &["jazz chorus", "jc-120", "jc120"]),
    ("Amp_CS100", &["jazz chorus", "jc-120", "jc120"]),
    ("Amp_CS120", &["jazz chorus", "jc-120", "jc120"]),
    ("Amp_EN30", &["ac30", "ac 30"]),
    ("Amp_EN50", &["ac50", "ac 50", "ac15", "ac 15"]),
    ("Amp_BT15", &["matchless", "badcat", "bad cat"]),
    ("Amp_BT30", &["matchless", "badcat", "bad cat"]),
    ("Amp_BT45", &["friedman", "bogner"]),
    ("Amp_CA38", &["mark i ", "mark iv", "mark v"]),
    ("Amp_CA85", &["mark ii", "mark iii"]),
    ("Amp_CA100", &["rectifier", "recto"]),
    ("Amp_GB38", &["jtm45", "jtm 45", "bluesbreaker"]),
    ("Amp_GB50", &["jmp", "super lead"]),
    ("Amp_GB100", &["laney"]),
    ("Amp_HG100", &["soldano", "slo"]),
    ("Amp_HG180", &["diezel", "engl"]),
    ("Amp_HG500", &["5150", "6505"]),
    ("Amp_MarshallPlexi", &["plexi", "super lead", "1959", "1987"]),
    ("Amp_MarshallJTM45", &["jtm45", "jtm 45"]),
    (
        "Amp_Marshall1962Bluesbreaker",
        &["bluesbreaker", "jtm45", "jtm 45"],
    ),
    ("Amp_MarshallJCM800", &["jcm800", "jcm 800"]),
    ("Amp_MarshallDSL15H", &["dsl"]),
    ("Amp_MarshallDSL100H", &["dsl"]),
    ("Amp_MarshallJVM410H", &["jvm"]),
    ("Amp_OrangeAD50", &["orange ad"]),
    ("Amp_OrangeOR50H", &["or50", "or 50", "or100"]),
    ("Amp_OrangeOR100", &["or100", "or 100", "or50"]),
    ("Amp_OrangeTinyTerror", &["tiny terror", "dual terror"]),
    ("Amp_OrangeRockerverb", &["rockerverb"]),
    ("Amp_OrangeJimmyBean", &["jim root"]),
    ("Bass_Amp_HT300B", &["svt-cl", "svt cl", "svt"]),
    ("Bass_Amp_HT100B", &["svt"]),
    ("Bass_Amp_LT25B", &["b-15", "b15", "b-18", "b18", "portaflex"]),
    ("Bass_Amp_LT85B", &["b-15", "b15", "b-18", "b18", "portaflex"]),
    ("Bass_Amp_CH300B", &["gallien", "rb800", "800rb"]),
    ("Bass_Amp_CH350B", &["gallien", "rb800", "800rb"]),
    ("Bass_Amp_CH600B", &["svt-4", "svt4"]),
    ("Bass_Amp_CS75B", &["hartke", "markbass", "mark bass"]),
    ("Bass_Amp_CS240B", &["hartke", "markbass", "mark bass"]),
    ("Bass_Amp_CS300B", &["hartke", "markbass", "mark bass"]),
    ("Bass_Amp_BT975B", &["aguilar"]),
    ("Bass_Amp_OrangeAD200B", &["ad200"]),
    ("Bass_Amp_EdenE300", &["eden"]),
    ("Bass_Amp_EdenWT550", &["eden"]),
    ("Bass_Amp_EdenWT800", &["eden"]),
];

/// The amp on the records, by artist (lower case), where it's well known and RS's tone stands in for it with something
/// else (Last Nite: RS has a Laney; the Strokes played Hot Rod DeVilles). Only a capture you have is picked.
pub const RECORDS: &[(&str, &[&str])] = &[
    ("the strokes", &["hot rod deville"]),
    ("rage against the machine", &["jcm800", "jcm 800"]),
    ("metallica", &["mark iic", "mark ii"]),
    ("stevie ray vaughan", &["super reverb", "vibroverb", "dumble"]),
    (
        "stevie ray vaughan & double trouble",
        &["super reverb", "vibroverb", "dumble"],
    ),
    ("jimi hendrix", &["plexi", "super lead"]),
    ("ac/dc", &["plexi", "super lead", "jtm45"]),
    ("van halen", &["plexi", "super lead"]),
    ("black sabbath", &["laney"]),
    ("u2", &["ac30", "ac 30"]),
    ("queen", &["ac30", "ac 30"]),
];

fn words_of(table: &'static [(&str, &'static [&'static str])], key: &str) -> &'static [&'static str] {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, w)| *w)
        .unwrap_or(&[])
}

/// The first of `words` a capture's gear or name says (lower case).
fn says(i: &Item, words: &'static [&'static str]) -> Option<&'static str> {
    let hay = format!("{} {} ", i.gear.as_deref().unwrap_or(""), i.name).to_lowercase();
    words.iter().copied().find(|w| hay.contains(w))
}

/// The capture for block `b` (from gear `key`, if known) on a bass tone or not.
pub fn pick<'a>(lib: &'a Library, b: &Block, key: Option<&str>, bass: bool) -> Option<&'a Item> {
    pick_why(lib, b, key, bass, &SongHint::default(), &|_| true).map(|p| p.0)
}

/// `pick` for a song, among the captures `usable` lets through (P38.2: one whose file loaded), and why (P41.1, D164):
/// a capture set up for the song; for an amp, the amp on the record (`RECORDS`, by the artist); the unit RS's gear
/// stands for (`Item::rs`; a cab by its body); for an amp, the real gear behind RS's (`REAL_GEAR`, by the capture's
/// own gear words); else one standing in for the same model of ours. Each time the nearest in gain, ties to the
/// lowest id.
pub fn pick_why<'a>(
    lib: &'a Library,
    b: &Block,
    key: Option<&str>,
    bass: bool,
    song: &SongHint,
    usable: &dyn Fn(&Item) -> bool,
) -> Option<(&'a Item, Why)> {
    let kind = kind_for(b)?;
    let id = b.def().id;
    let body = key.filter(|_| kind == CKind::Ir).map(rs_gear::cab_body);
    let of_unit = |i: &&Item| {
        key.is_some_and(|k| i.rs.iter().any(|r| r == k))
            || body.is_some_and(|bd| i.rs.iter().any(|r| r.eq_ignore_ascii_case(bd)))
    };
    let of_model = |i: &&Item| i.skin.as_deref() == Some(id);
    let fits = |i: &&Item| {
        i.kind == kind && (kind == CKind::Pedal || i.bass == bass || id == "amp_bass") && usable(i)
    };
    let nearest = |c: Vec<&'a Item>| {
        let g = gain_of(b).unwrap_or(5.0);
        c.into_iter().min_by(|x, y| {
            let dx = (x.gain.unwrap_or(5.0) - g).abs();
            let dy = (y.gain.unwrap_or(5.0) - g).abs();
            dx.total_cmp(&dy).then(x.id.cmp(&y.id))
        })
    };
    let all = || lib.items.iter().filter(fits);
    if !song.title.is_empty()
        && let Some(i) = nearest(all().filter(|i| i.songs.contains(&song.title)).collect())
    {
        return Some((i, Why::Song));
    }
    if kind == CKind::Nam && !bass {
        let words = words_of(RECORDS, &song.artist);
        if let Some(i) = nearest(all().filter(|i| says(i, words).is_some()).collect()) {
            return Some((i, Why::Record(says(i, words).unwrap_or(""))));
        }
    }
    if let Some(i) = nearest(all().filter(of_unit).collect()) {
        return Some((i, Why::Unit));
    }
    if kind == CKind::Nam
        && let Some(k) = key
    {
        let words = words_of(REAL_GEAR, k);
        if let Some(i) = nearest(all().filter(|i| says(i, words).is_some()).collect()) {
            return Some((i, Why::Gear(says(i, words).unwrap_or(""))));
        }
    }
    nearest(all().filter(of_model).collect()).map(|i| (i, Why::Model))
}

/// The song tone's EQ carried onto a capture's post-EQ (P41.1, D164; the tech's #6): our amp's Bass, Mid and Treble as
/// dB from noon (1.2 dB a step: a capture is one setting of the real knobs, usually at noon), a tweed's Tone as its
/// treble, an AC30's Cut taking treble off; a bass head's dB stack at half. Within the capture's +-12 dB.
pub fn song_eq(ours: &Block) -> [f32; 3] {
    let d = |k: &str| ours.get(k).map(|v| (v - 5.0) * 1.2);
    let [bass, mid, mut treble] = match ours.def().id {
        "amp_tweed" => [None, None, d("tone")],
        "amp_bass_ss" => {
            let h = |k: &str| ours.get(k).map(|v| v * 0.5);
            [h("bass"), h("mid"), h("treble")]
        }
        _ => [d("bass"), d("mid"), d("treble")],
    };
    if ours.def().id == "amp_chime"
        && let Some(c) = ours.get("cut")
    {
        // (the Cut's default, 3, is the voicing; turned up it takes the top off)
        treble = Some(treble.unwrap_or(0.0) - (c - 3.0) * 0.8);
    }
    let r = |v: Option<f32>| ((v.unwrap_or(0.0) * 2.0).round() / 2.0).clamp(-12.0, 12.0);
    [r(bass), r(mid), r(treble)]
}

/// Why the captures on the song tones' boards were picked: (the tone's key, the capture, the line), for the last boards
/// made (`nam_board_song`); the room reads it (`why_of`).
static WHY: std::sync::Mutex<Vec<(String, u32, String)>> = std::sync::Mutex::new(Vec::new());

/// Why capture `asset` is on song tone `tone_key`'s board (P41.1, D164), when a NAM board picked it.
pub fn why_of(tone_key: &str, asset: u32) -> Option<String> {
    let w = WHY.lock().unwrap_or_else(|e| e.into_inner());
    w.iter()
        .rev()
        .find(|(k, a, _)| k == tone_key && *a == asset)
        .map(|(_, _, l)| l.clone())
}

fn keep_why(tone_key: &str, lines: Vec<(u32, String)>) {
    let mut w = WHY.lock().unwrap_or_else(|e| e.into_inner());
    w.retain(|(k, _, _)| k != tone_key);
    w.extend(lines.into_iter().map(|(a, l)| (tone_key.to_string(), a, l)));
    let over = w.len().saturating_sub(512);
    w.drain(..over);
}

/// Whether a capture's file is loaded (registered with the board): one that isn't would play nothing.
fn loaded(i: &Item) -> bool {
    qrs_audio::board::nam::is_registered(i.id)
}

/// A song tone as a NAM board (our board where the library has nothing for a block).
pub fn nam_board(tone: &RigTone, lib: &Library) -> Board {
    nam_board_with(tone, lib, &loaded)
}

/// `nam_board` for a song: its own captures and the amp on its record first (P41.1).
pub fn nam_board_for(tone: &RigTone, song: &SongHint, lib: &Library) -> Board {
    nam_board_song(tone, song, lib, &loaded)
}

/// `nam_board` with the captures `usable` lets through (tests: files never registered).
pub fn nam_board_with(tone: &RigTone, lib: &Library, usable: &dyn Fn(&Item) -> bool) -> Board {
    nam_board_song(tone, &SongHint::default(), lib, usable)
}

/// `nam_board_for` with the captures `usable` lets through.
pub fn nam_board_song(
    tone: &RigTone,
    song: &SongHint,
    lib: &Library,
    usable: &dyn Fn(&Item) -> bool,
) -> Board {
    let (board, keys) = rs_gear::board_keyed(tone);
    let (b, why) = swap(board, &keys, rs_gear::is_bass_tone(tone), lib, song, usable);
    keep_why(&tone.key, why);
    b
}

/// One of our boards (a tone kept as our models, the gear index's) as a NAM board, by model alone.
pub fn nam_of(board: Board, bass: bool, lib: &Library) -> Board {
    let keys = vec![None; board.blocks.len()];
    swap(board, &keys, bass, lib, &SongHint::default(), &loaded).0
}

/// Our blocks swapped for captures. (P38.2, D153) The unit of ours a capture replaces stays on the board, off,
/// right after it: MODELLED / NAM flips between the two, the song's own model and knobs are there to flip to, and a
/// capture whose file goes missing falls back to it (`captures::heal`). A capture with its cab in it turns our cab
/// off (kept, for MODELLED). A full board (`MAX_BLOCKS`) keeps the capture alone.
fn swap(
    mut board: Board,
    keys: &[Option<String>],
    bass: bool,
    lib: &Library,
    song: &SongHint,
    usable: &dyn Fn(&Item) -> bool,
) -> (Board, Vec<(u32, String)>) {
    let mut why = Vec::new();
    let mut captures = 0;
    let mut cab_inside = false;
    let mut out: Vec<(Block, bool)> = Vec::with_capacity(board.blocks.len() * 2);
    for (b, key) in board.blocks.iter().zip(keys.iter()) {
        let k = kind_for(b);
        let costly = matches!(k, Some(CKind::Nam | CKind::Pedal));
        let item = (!costly || captures < MAX_CAPTURES)
            .then(|| pick_why(lib, b, key.as_deref(), bass, song, usable))
            .flatten();
        match item {
            Some((it, w)) => {
                let mut nb = Block::of(it.kind.model()).expect("file model");
                nb.set_asset(it.id);
                nb.on = b.on;
                why.push((it.id, w.line(key.as_deref(), b, it)));
                // (P41.1: the song's EQ on the capture's post-EQ, not the capture's defaults)
                if it.kind == CKind::Nam && b.def().kind == Kind::Amp {
                    let [lo, mi, hi] = song_eq(b);
                    nb.set("bass", lo).set("mid", mi).set("treble", hi);
                }
                if costly {
                    captures += 1;
                }
                // (the library's word, else the file's own "has a cab" metadata, P38.2)
                let cab_in = it
                    .cab
                    .or_else(|| qrs_audio::board::nam::capture_info(it.id).and_then(|i| i.includes_cab));
                if it.kind == CKind::Nam && cab_in == Some(true) && b.on {
                    cab_inside = true;
                }
                out.push((nb, false));
                let mut ours = *b;
                ours.on = false;
                out.push((ours, true));
            }
            None => out.push((*b, false)),
        }
    }
    // (over the board's size: the kept units of ours go, the drives' first, then the cabs', then the amps')
    for kind in [Kind::Drive, Kind::Fuzz, Kind::Cab, Kind::Amp] {
        while out.len() > MAX_BLOCKS {
            match out.iter().rposition(|(b, kept)| *kept && b.def().kind == kind) {
                Some(i) => {
                    out.remove(i);
                }
                None => break,
            }
        }
    }
    let mut blocks: Vec<Block> = out.into_iter().map(|(b, _)| b).collect();
    if cab_inside {
        // (the capture has its cab: no second one; ours stays, off, for MODELLED)
        for b in blocks.iter_mut() {
            if b.def().kind == Kind::Cab {
                b.on = false;
            }
        }
    }
    blocks.truncate(MAX_BLOCKS);
    board.blocks = blocks;
    (board, why)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_formats::rig::Gear;

    fn item(id: u32, kind: CKind, skin: &str, rs: &[&str], gain: f32, cab: Option<bool>) -> Item {
        let mut i = super::super::blank(kind);
        i.id = id;
        i.skin = Some(skin.into());
        i.rs = rs.iter().map(|s| s.to_string()).collect();
        i.gain = Some(gain);
        i.cab = cab;
        i
    }

    fn lib(items: Vec<Item>) -> Library {
        Library {
            items,
            ..Default::default()
        }
    }

    fn tone(gear: Vec<(&str, &str, &[(&str, f32)])>) -> RigTone {
        RigTone {
            key: "t".into(),
            name: "t".into(),
            descriptors: vec![],
            volume_db: -20.0,
            gear: gear
                .into_iter()
                .map(|(slot, key, knobs)| Gear {
                    slot: slot.into(),
                    key: key.into(),
                    knobs: knobs.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_unit_first_then_the_model_at_the_nearest_gain() {
        let l = lib(vec![
            item(1, CKind::Nam, "amp_lead", &[], 3.0, Some(false)),
            item(2, CKind::Nam, "amp_lead", &[], 8.0, Some(false)),
            item(
                3,
                CKind::Nam,
                "amp_lead",
                &["Amp_MarshallJCM800"],
                6.0,
                Some(false),
            ),
            item(4, CKind::Ir, "cab_4x12", &["GB412CMKII"], 5.0, None),
            item(5, CKind::Ir, "cab_4x12", &[], 5.0, None),
        ]);
        // a JCM800 at gain 8 into a GB 4x12: the JCM800 capture (though another is nearer in gain), the GB IR
        let t = tone(vec![
            ("Amp", "Amp_MarshallJCM800", &[("Gain", 80.0)]),
            ("Cabinet", "Cab_GB412CMKII_57_Cap", &[]),
        ]);
        let b = nam_board_with(&t, &l, &|_| true);
        let ids: Vec<(&str, u32)> = b.blocks.iter().map(|x| (x.def().id, x.asset)).collect();
        assert!(ids.contains(&("amp_nam", 3)), "{ids:?}");
        assert!(ids.contains(&("cab_ir", 4)), "{ids:?}");
        // an amp of the same model the library has no unit for: the nearest gain
        let t = tone(vec![("Amp", "Amp_AT120", &[("Gain", 75.0)])]);
        let b = nam_board_with(&t, &l, &|_| true);
        assert!(
            b.blocks.iter().any(|x| x.def().id == "amp_nam" && x.asset == 2),
            "{:?}",
            b.to_json()
        );
    }

    #[test]
    fn a_capture_with_its_cab_takes_the_cabs_place_and_nothing_found_stays_ours() {
        let l = lib(vec![item(
            1,
            CKind::Nam,
            "amp_lead",
            &["Amp_MarshallJCM800"],
            8.0,
            Some(true),
        )]);
        let t = tone(vec![
            ("Amp", "Amp_MarshallJCM800", &[("Gain", 80.0)]),
            ("Cabinet", "Cab_GB412CMKII_57_Cap", &[]),
        ]);
        let b = nam_board_with(&t, &l, &|_| true);
        assert!(b.blocks.iter().any(|x| x.def().id == "amp_nam"));
        assert!(
            !b.blocks.iter().any(|x| x.on && x.def().kind == Kind::Cab),
            "{:?}",
            b.to_json()
        );
        // an empty library: the board as before
        let b0 = nam_board(&t, &lib(vec![]));
        assert_eq!(
            b0.blocks.iter().map(|x| x.def().id).collect::<Vec<_>>(),
            rs_gear::board_for(&t)
                .blocks
                .iter()
                .map(|x| x.def().id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_unit_of_ours_stays_off_beside_its_capture_and_a_missing_file_is_never_picked() {
        let l = lib(vec![
            item(
                1,
                CKind::Nam,
                "amp_lead",
                &["Amp_MarshallJCM800"],
                8.0,
                Some(false),
            ),
            item(2, CKind::Ir, "cab_4x12", &["GB412CMKII"], 5.0, None),
        ]);
        let t = tone(vec![
            ("Amp", "Amp_MarshallJCM800", &[("Gain", 80.0)]),
            ("Cabinet", "Cab_GB412CMKII_57_Cap", &[]),
        ]);
        let ours = rs_gear::board_for(&t);
        let b = nam_board_with(&t, &l, &|_| true);
        let ids: Vec<(&str, bool)> = b.blocks.iter().map(|x| (x.def().id, x.on)).collect();
        // capture on, our amp off right after it; the IR on, our cab off after it
        let amp = ours.blocks.iter().find(|x| x.def().kind == Kind::Amp).unwrap();
        let i = b.blocks.iter().position(|x| x.def().id == "amp_nam").unwrap();
        assert!(b.blocks[i].on);
        assert_eq!(b.blocks[i + 1].model, amp.model, "{ids:?}");
        assert!(!b.blocks[i + 1].on);
        assert_eq!(b.blocks[i + 1].v, amp.v, "the song's knobs kept");
        let j = b.blocks.iter().position(|x| x.def().id == "cab_ir").unwrap();
        assert!(b.blocks[j].on && !b.blocks[j + 1].on && b.blocks[j + 1].def().kind == Kind::Cab);
        // a capture whose file didn't load: ours, as if the library didn't have it
        let b = nam_board_with(&t, &l, &|i| i.id != 1);
        assert!(
            !b.blocks.iter().any(|x| x.def().id == "amp_nam"),
            "{:?}",
            b.to_json()
        );
        assert!(b.blocks.iter().any(|x| x.on && x.model == amp.model));
    }

    #[test]
    fn a_full_board_keeps_the_captures_and_drops_kept_units_first() {
        let l = lib(vec![item(
            1,
            CKind::Nam,
            "amp_lead",
            &["Amp_MarshallJCM800"],
            8.0,
            Some(false),
        )]);
        let mut gear: Vec<(&str, &str, &[(&str, f32)])> =
            vec![("Amp", "Amp_MarshallJCM800", &[("Gain", 80.0)])];
        // (an amp, 10 EQs and the cab every electric tone gets: a full board)
        for _ in 0..10 {
            gear.push(("Pedal", "Pedal_EQ5", &[]));
        }
        let t = tone(gear);
        let b = nam_board_with(&t, &l, &|_| true);
        assert!(b.blocks.len() <= MAX_BLOCKS);
        assert!(b.blocks.iter().any(|x| x.on && x.def().id == "amp_nam"));
        assert_eq!(b.blocks.iter().filter(|x| x.def().id == "eq5").count(), 10);
    }

    fn named(id: u32, skin: &str, gear: &str, gain: f32) -> Item {
        let mut i = item(id, CKind::Nam, skin, &[], gain, Some(false));
        i.gear = Some(gear.into());
        i.name = format!("{gear} \u{b7} a setting");
        i
    }

    /// P41.1 (D164): a Dual Rec song gets the Recto capture, not the Diezel nearer in gain; the Strokes get the Hot Rod
    /// DeVille over RS's Laney; each says why.
    #[test]
    fn captures_are_matched_by_the_real_gear_and_say_why() {
        let l = lib(vec![
            named(1, "amp_modern", "Diezel VH4", 8.0),
            named(2, "amp_modern", "Mesa Boogie Single Rectifier 50", 6.0),
            named(3, "amp_lead", "Laney Ironheart IRT120", 3.0),
            named(4, "amp_clean", "Fender Hot Rod DeVille", 3.0),
        ]);
        let mut t = tone(vec![
            ("Amp", "Amp_CA100", &[("Gain", 85.0)]),
            ("Cabinet", "Cab_CA412C_57_Cone", &[]),
        ]);
        t.key = "DualRec_dist".into();
        let b = nam_board_song(&t, &SongHint::default(), &l, &|_| true);
        assert!(
            b.blocks.iter().any(|x| x.def().id == "amp_nam" && x.asset == 2),
            "{:?}",
            b.to_json()
        );
        let why = why_of("DualRec_dist", 2).unwrap();
        assert!(why.contains("rectifier") && why.contains("Cali-100"), "{why}");
        // Last Nite: RS's Laney, the record's Hot Rod DeVille
        let mut t = tone(vec![
            ("Amp", "Amp_GB100", &[("Gain", 30.0)]),
            ("Cabinet", "Cab_TW112C_57_Edge", &[]),
        ]);
        t.key = "LastNite_clean".into();
        let b = nam_board_song(&t, &SongHint::new("The Strokes", "Last Nite"), &l, &|_| true);
        assert!(
            b.blocks.iter().any(|x| x.def().id == "amp_nam" && x.asset == 4),
            "{:?}",
            b.to_json()
        );
        assert!(why_of("LastNite_clean", 4).unwrap().contains("record"));
        // another band on the same Laney: the Laney capture, by its gear words
        let b = nam_board_song(&t, &SongHint::new("Someone", "Else"), &l, &|_| true);
        assert!(
            b.blocks.iter().any(|x| x.def().id == "amp_nam" && x.asset == 3),
            "{:?}",
            b.to_json()
        );
    }

    /// P41.1 (D164): the capture's post-EQ gets the song's tone stack (it was left at 0 dB).
    #[test]
    fn a_capture_carries_the_songs_eq() {
        let l = lib(vec![item(
            1,
            CKind::Nam,
            "amp_lead",
            &["Amp_MarshallJCM800"],
            8.0,
            Some(false),
        )]);
        let t = tone(vec![(
            "Amp",
            "Amp_MarshallJCM800",
            &[("Gain", 80.0), ("Bass", 20.0), ("Mid", 90.0), ("Treble", 50.0)],
        )]);
        let b = nam_board_with(&t, &l, &|_| true);
        let c = b.blocks.iter().find(|x| x.def().id == "amp_nam").unwrap();
        assert_eq!(c.get("bass"), Some(-3.5));
        assert_eq!(c.get("mid"), Some(5.0));
        assert_eq!(c.get("treble"), Some(0.0));
        let tw = Block::of("amp_tweed").unwrap().with("tone", 10.0);
        assert_eq!(song_eq(&tw), [0.0, 0.0, 6.0]);
    }
}
