//! Suggested captures (P36.7, D96; new code, from `tools/nam/harvest.py`'s targets): the real units behind our
//! RS gear mapping, ranked by how many songs in your library use that gear. The first-time setup's "Use
//! NAM?" page and Settings > Tone list them; each is fetched through your own TONE3000 account (the select flow in
//! `tone3000.rs`), never by quickRS itself: we neither download nor bundle TONE3000 content (owner decision
//! 2026-11-05). The table is only names, our model each stands in for, RS's gear keys and the words to
//! search TONE3000 for.

use qrs_audio::board::rs_gear;

use super::Kind;

/// Whether this build lists the suggestions: they are real units by name, so only with brand names on (D58); the
/// shared build has NAM and TONE3000 without them (any capture, picked from an amp's Captures).
pub const SHOWN: bool = qrs_audio::board::catalog::SHOW_BRANDS;

/// A real unit worth a capture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unit {
    /// Its name ("Fender Deluxe Reverb").
    pub name: &'static str,
    pub kind: Kind,
    /// Our model it stands in for.
    pub skin: &'static str,
    /// RS's gear keys for it (a cab: the cab's body, "TW110C").
    pub rs: &'static [&'static str],
    /// What to search TONE3000 for.
    pub query: &'static str,
    pub bass: bool,
}

impl Unit {
    /// Whether a song's gear key is this unit.
    pub fn is(&self, key: &str) -> bool {
        match self.kind {
            Kind::Ir => {
                let body = rs_gear::cab_body(key);
                self.rs.iter().any(|r| r.eq_ignore_ascii_case(body))
            }
            _ => self.rs.contains(&key),
        }
    }
}

/// A suggestion: the unit, how many of your songs use it, and whether your library has a capture of it already.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub unit: &'static Unit,
    pub songs: usize,
    pub have: bool,
}

/// The units used by `songs` (each song's gear keys), most songs first (ties by name), at most `limit`; units
/// with the same gear keys only once; and how many songs the list covers (a song using any of them). `bass`: only
/// bass gear (`songs` are then the bass parts' gear), else only guitar gear (P37.6a: an SVT is no use to a
/// guitarist, a Twin none to a bassist).
pub fn rank(
    songs: &[Vec<String>],
    have: impl Fn(&Unit) -> bool,
    limit: usize,
    bass: bool,
) -> (Vec<Suggestion>, usize) {
    let mut seen: Vec<(Kind, &[&str])> = Vec::new();
    let mut out: Vec<Suggestion> = Vec::new();
    for u in UNITS.iter().filter(|u| u.bass == bass) {
        if seen.iter().any(|(k, rs)| *k == u.kind && *rs == u.rs) {
            continue;
        }
        seen.push((u.kind, u.rs));
        let n = songs.iter().filter(|g| g.iter().any(|k| u.is(k))).count();
        if n > 0 {
            out.push(Suggestion {
                unit: u,
                songs: n,
                have: have(u),
            });
        }
    }
    out.sort_by(|a, b| b.songs.cmp(&a.songs).then(a.unit.name.cmp(b.unit.name)));
    out.truncate(limit);
    let covered = songs
        .iter()
        .filter(|g| g.iter().any(|k| out.iter().any(|s| s.unit.is(k))))
        .count();
    (out, covered)
}

/// The suggested captures for this library (the gear index's songs read so far) and what you play (a bass: the bass
/// parts' gear, bass units only): at most `limit`, how many of your songs they cover and how many songs were
/// counted (those with a part for the instrument).
pub fn suggested_captures(
    limit: usize,
    inst: qrs_analysis::instrument::Instrument,
) -> (Vec<Suggestion>, usize, usize) {
    let songs = crate::gearindex::song_gear(inst.is_bass());
    if !SHOWN {
        return (Vec::new(), 0, songs.len());
    }
    let have = |u: &Unit| {
        super::with(|l| {
            l.items
                .iter()
                .any(|i| i.rs.iter().any(|r| u.rs.contains(&r.as_str())))
        })
    };
    let (list, covered) = rank(&songs, have, limit, inst.is_bass());
    (list, covered, songs.len())
}

/// TONE3000's search for a unit's words (the amp room's "Suggested captures" opens it in the browser; the user picks
/// and downloads there). `/search?q=word+word` is the site's own search page (checked 2026-11-05, P37.4: its
/// canonical link and title echo the words, and the results match; `&gears=amp|pedal|cab` would narrow it).
pub fn search_url(query: &str) -> String {
    let q: Vec<String> = query
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    format!("https://www.tone3000.com/search?q={}", q.join("+"))
}

/// The units (amps, pedals, cabs), as `tools/nam/harvest.py` targets them.
pub const UNITS: &[Unit] = &[
    Unit {
        name: "Fender Deluxe Reverb",
        kind: Kind::Nam,
        skin: "amp_clean",
        rs: &["Amp_AT20"],
        query: "Fender Deluxe Reverb",
        bass: false,
    },
    Unit {
        name: "Fender Twin Reverb",
        kind: Kind::Nam,
        skin: "amp_clean",
        rs: &["Amp_TW26"],
        query: "Fender Twin Reverb",
        bass: false,
    },
    Unit {
        name: "Fender Princeton Reverb",
        kind: Kind::Nam,
        skin: "amp_clean",
        rs: &["Amp_TW26"],
        query: "Fender Princeton Reverb",
        bass: false,
    },
    Unit {
        name: "Fender Hot Rod Deville",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_AT120"],
        query: "Fender Hot Rod",
        bass: false,
    },
    Unit {
        name: "Matchless DC-30",
        kind: Kind::Nam,
        skin: "amp_chime",
        rs: &["Amp_BT15", "Amp_BT30"],
        query: "Matchless DC30",
        bass: false,
    },
    Unit {
        name: "Friedman BE-100",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_BT45"],
        query: "Friedman BE100",
        bass: false,
    },
    Unit {
        name: "Bogner Ecstasy",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_BT45"],
        query: "Bogner Ecstasy",
        bass: false,
    },
    Unit {
        name: "Mesa/Boogie Mark V",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_CA38"],
        query: "Mesa Boogie Mark V",
        bass: false,
    },
    Unit {
        name: "Mesa/Boogie Mark IIC+",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_CA85"],
        query: "Mesa Boogie Mark IIC+",
        bass: false,
    },
    Unit {
        name: "Mesa/Boogie Dual Rectifier",
        kind: Kind::Nam,
        skin: "amp_modern",
        rs: &["Amp_CA100"],
        query: "Mesa Dual Rectifier",
        bass: false,
    },
    Unit {
        name: "Roland JC-120",
        kind: Kind::Nam,
        skin: "amp_clean",
        rs: &["Amp_CS90", "Amp_CS100", "Amp_CS120"],
        query: "Roland JC-120",
        bass: false,
    },
    Unit {
        name: "Vox AC30",
        kind: Kind::Nam,
        skin: "amp_chime",
        rs: &["Amp_EN30"],
        query: "Vox AC30",
        bass: false,
    },
    Unit {
        name: "Vox AC50",
        kind: Kind::Nam,
        skin: "amp_chime",
        rs: &["Amp_EN50"],
        query: "Vox AC50",
        bass: false,
    },
    Unit {
        name: "Marshall JMP 2204",
        kind: Kind::Nam,
        skin: "amp_crunch",
        rs: &["Amp_GB50"],
        query: "Marshall JMP",
        bass: false,
    },
    Unit {
        name: "Laney AOR",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_GB100"],
        query: "Laney AOR",
        bass: false,
    },
    Unit {
        name: "Soldano SLO-100",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_HG100"],
        query: "Soldano SLO",
        bass: false,
    },
    Unit {
        name: "Diezel VH4",
        kind: Kind::Nam,
        skin: "amp_modern",
        rs: &["Amp_HG180"],
        query: "Diezel VH4",
        bass: false,
    },
    Unit {
        name: "Peavey 5150",
        kind: Kind::Nam,
        skin: "amp_modern",
        rs: &["Amp_HG500"],
        query: "Peavey 5150",
        bass: false,
    },
    Unit {
        name: "Fender 5E3 Tweed Deluxe",
        kind: Kind::Nam,
        skin: "amp_tweed",
        rs: &["Amp_TW22"],
        query: "Fender Tweed Deluxe",
        bass: false,
    },
    Unit {
        name: "Fender Tweed Bassman",
        kind: Kind::Nam,
        skin: "amp_tweed",
        rs: &["Amp_TW40"],
        query: "Fender Bassman 5F6",
        bass: false,
    },
    Unit {
        name: "Gibson GA vintage",
        kind: Kind::Nam,
        skin: "amp_tweed",
        rs: &["Amp_GibsonGA8", "Amp_GibsonGA79", "Amp_GibsonGA88"],
        query: "Gibson GA",
        bass: false,
    },
    Unit {
        name: "Epiphone vintage",
        kind: Kind::Nam,
        skin: "amp_tweed",
        rs: &[
            "Amp_EpiphoneElectarCentury",
            "Amp_EpiphoneElectarM",
            "Amp_EpiphoneElectricHawaiian",
            "Amp_EpiphoneZephyr",
        ],
        query: "Epiphone",
        bass: false,
    },
    Unit {
        name: "Marshall Super Lead Plexi",
        kind: Kind::Nam,
        skin: "amp_crunch",
        rs: &["Amp_MarshallPlexi"],
        query: "Marshall Plexi",
        bass: false,
    },
    Unit {
        name: "Marshall JTM45",
        kind: Kind::Nam,
        skin: "amp_crunch",
        rs: &["Amp_MarshallJTM45", "Amp_Marshall1962Bluesbreaker", "Amp_GB38"],
        query: "Marshall JTM45",
        bass: false,
    },
    Unit {
        name: "Marshall JCM800",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_MarshallJCM800"],
        query: "Marshall JCM800",
        bass: false,
    },
    Unit {
        name: "Marshall DSL",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_MarshallDSL15H", "Amp_MarshallDSL100H"],
        query: "Marshall DSL",
        bass: false,
    },
    Unit {
        name: "Marshall JVM410",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_MarshallJVM410H"],
        query: "Marshall JVM",
        bass: false,
    },
    Unit {
        name: "Orange AD30/AD50",
        kind: Kind::Nam,
        skin: "amp_crunch",
        rs: &["Amp_OrangeAD50"],
        query: "Orange AD30",
        bass: false,
    },
    Unit {
        name: "Orange OR50/OR100",
        kind: Kind::Nam,
        skin: "amp_crunch",
        rs: &["Amp_OrangeOR50H", "Amp_OrangeOR100"],
        query: "Orange OR50",
        bass: false,
    },
    Unit {
        name: "Orange Tiny Terror",
        kind: Kind::Nam,
        skin: "amp_crunch",
        rs: &["Amp_OrangeTinyTerror"],
        query: "Orange Tiny Terror",
        bass: false,
    },
    Unit {
        name: "Orange Rockerverb",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &["Amp_OrangeRockerverb"],
        query: "Orange Rockerverb",
        bass: false,
    },
    Unit {
        name: "Orange Jim Root Terror",
        kind: Kind::Nam,
        skin: "amp_modern",
        rs: &["Amp_OrangeJimmyBean"],
        query: "Orange Jim Root",
        bass: false,
    },
    Unit {
        name: "Ampeg SVT",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &["Bass_Amp_HT300B", "Bass_Amp_CH600B"],
        query: "Ampeg SVT",
        bass: true,
    },
    Unit {
        name: "Ampeg B-15 Portaflex",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &["Bass_Amp_LT25B", "Bass_Amp_LT85B"],
        query: "Ampeg B-15",
        bass: true,
    },
    Unit {
        name: "Gallien-Krueger 800RB",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &["Bass_Amp_CH300B"],
        query: "Gallien Krueger",
        bass: true,
    },
    Unit {
        name: "Eden WT800",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &["Bass_Amp_EdenE300", "Bass_Amp_EdenWT550", "Bass_Amp_EdenWT800"],
        query: "Eden WT",
        bass: true,
    },
    Unit {
        name: "Hartke",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &["Bass_Amp_CS75B"],
        query: "Hartke",
        bass: true,
    },
    Unit {
        name: "Fender Bassman (bass)",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &[],
        query: "Fender Bassman bass",
        bass: true,
    },
    Unit {
        name: "Orange bass amp",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &["Bass_Amp_OrangeAD200B"],
        query: "Orange AD200",
        bass: true,
    },
    Unit {
        name: "Markbass",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &[],
        query: "Markbass",
        bass: true,
    },
    Unit {
        name: "Ibanez TS808",
        kind: Kind::Pedal,
        skin: "green",
        rs: &["Pedal_CustomDrive", "Pedal_SuperDrive"],
        query: "Ibanez TS808",
        bass: false,
    },
    Unit {
        name: "Boss SD-1",
        kind: Kind::Pedal,
        skin: "green",
        rs: &["Pedal_SuperDrive"],
        query: "Boss SD-1",
        bass: false,
    },
    Unit {
        name: "Klon Centaur",
        kind: Kind::Pedal,
        skin: "gold",
        rs: &["Pedal_LineDrive"],
        query: "Klon Centaur",
        bass: false,
    },
    Unit {
        name: "ProCo RAT",
        kind: Kind::Pedal,
        skin: "rodent",
        rs: &["Pedal_VintageDistortion"],
        query: "ProCo RAT",
        bass: false,
    },
    Unit {
        name: "Electro-Harmonix Big Muff Pi",
        kind: Kind::Pedal,
        skin: "big_pi",
        rs: &["Pedal_BuzzToo", "Pedal_FuzzWasHe", "Bass_Pedal_BassFuzz"],
        query: "Big Muff",
        bass: false,
    },
    Unit {
        name: "Fuzz Face (germanium)",
        kind: Kind::Pedal,
        skin: "round_fuzz",
        rs: &["Pedal_CaptFuzzle"],
        query: "Fuzz Face germanium",
        bass: false,
    },
    Unit {
        name: "Germanium overdrive",
        kind: Kind::Pedal,
        skin: "germanium",
        rs: &["Pedal_GermaniumDrive"],
        query: "germanium overdrive",
        bass: false,
    },
    Unit {
        name: "Dallas Rangemaster",
        kind: Kind::Pedal,
        skin: "treble_boost",
        rs: &["Pedal_RangeBooster"],
        query: "Rangemaster",
        bass: false,
    },
    Unit {
        name: "Boss DS-1",
        kind: Kind::Pedal,
        skin: "orange",
        rs: &["Pedal_Distortion"],
        query: "Boss DS-1",
        bass: false,
    },
    Unit {
        name: "Marshall Guv'nor",
        kind: Kind::Pedal,
        skin: "orange",
        rs: &["Pedal_MarshallGuvnorPlus"],
        query: "Marshall Guvnor",
        bass: false,
    },
    Unit {
        name: "Boss MT-2 Metal Zone",
        kind: Kind::Pedal,
        skin: "metal",
        rs: &["Pedal_MetalDistortion", "Pedal_ShredZone"],
        query: "Boss MT-2",
        bass: false,
    },
    Unit {
        name: "Boss HM-2",
        kind: Kind::Pedal,
        skin: "metal",
        rs: &["Pedal_MetalDistortion"],
        query: "Boss HM-2",
        bass: false,
    },
    Unit {
        name: "Boss BD-2 Blues Driver",
        kind: Kind::Pedal,
        skin: "blues",
        rs: &[],
        query: "Boss Blues Driver",
        bass: false,
    },
    Unit {
        name: "Fulltone OCD",
        kind: Kind::Pedal,
        skin: "orange",
        rs: &[],
        query: "Fulltone OCD",
        bass: false,
    },
    Unit {
        name: "Tone Bender MkII",
        kind: Kind::Pedal,
        skin: "bender",
        rs: &["Pedal_BuzzOne"],
        query: "Tone Bender",
        bass: false,
    },
    Unit {
        name: "SansAmp Bass Driver DI",
        kind: Kind::Pedal,
        skin: "bass_drive",
        rs: &[
            "DI_Amp_BassDriver",
            "Bass_Pedal_BassOverdrive",
            "Bass_Pedal_BassDistortion",
        ],
        query: "SansAmp Bass Driver",
        bass: true,
    },
    Unit {
        name: "Darkglass B7K",
        kind: Kind::Pedal,
        skin: "bass_drive",
        rs: &["Bass_Pedal_BassOverdrive", "Bass_Pedal_BassDistortion"],
        query: "Darkglass B7K",
        bass: true,
    },
    Unit {
        name: "Marshall 1960A 4x12 (G12M greenback)",
        kind: Kind::Ir,
        skin: "cab_4x12",
        rs: &[
            "GB412CMKI",
            "GB412CMKII",
            "GB412CMKIII",
            "Marshall1960a",
            "Marshall1960AX",
            "Marshall1960TV",
            "EN4120C",
        ],
        query: "Celestion Greenback 4x12",
        bass: false,
    },
    Unit {
        name: "Marshall 1960 4x12 (G12T-75)",
        kind: Kind::Ir,
        skin: "cab_4x12",
        rs: &["GB412CMKIII", "Marshall1960a"],
        query: "Marshall 1960 G12T-75",
        bass: false,
    },
    Unit {
        name: "Mesa Rectifier 4x12 (V30)",
        kind: Kind::Ir,
        skin: "cab_4x12",
        rs: &["CA412C", "HG212C"],
        query: "Mesa 4x12",
        bass: false,
    },
    Unit {
        name: "Orange PPC412",
        kind: Kind::Ir,
        skin: "cab_4x12",
        rs: &["OrangePPC412", "OrangeJimmyBean"],
        query: "Orange PPC412",
        bass: false,
    },
    Unit {
        name: "Orange PPC212",
        kind: Kind::Ir,
        skin: "cab_2x12",
        rs: &["OrangePPC212OB"],
        query: "Orange PPC212",
        bass: false,
    },
    Unit {
        name: "Greenback 2x12 (1936-style)",
        kind: Kind::Ir,
        skin: "cab_4x12",
        rs: &["Marshall1936"],
        query: "Marshall 1936",
        bass: false,
    },
    Unit {
        name: "Vox AC30 2x12 (Alnico Blue)",
        kind: Kind::Ir,
        skin: "cab_2x12",
        rs: &["EN212C"],
        query: "Vox AC30 cab",
        bass: false,
    },
    Unit {
        name: "Roland JC-120 2x12",
        kind: Kind::Ir,
        skin: "cab_2x12",
        rs: &["CS212C"],
        query: "JC-120 cab",
        bass: false,
    },
    Unit {
        name: "Fender Twin 2x12",
        kind: Kind::Ir,
        skin: "cab_2x12",
        rs: &["HG2120C"],
        query: "Fender Twin cab",
        bass: false,
    },
    Unit {
        name: "Fender Bassman 4x10 (open back)",
        kind: Kind::Ir,
        skin: "cab_2x12",
        rs: &["TW410C", "BT410C"],
        query: "Fender Bassman 4x10",
        bass: false,
    },
    Unit {
        name: "Fender Deluxe 1x12",
        kind: Kind::Ir,
        skin: "cab_1x12",
        rs: &[
            "AT0112C", "AT1121C", "TW112C", "CA112C", "BT1120C", "BT1121C", "CS1120C",
        ],
        query: "Fender Deluxe Reverb cab",
        bass: false,
    },
    Unit {
        name: "Fender Princeton 1x10",
        kind: Kind::Ir,
        skin: "cab_1x10",
        rs: &["TW110C"],
        query: "Fender Princeton cab",
        bass: false,
    },
    Unit {
        name: "Mesa 2x15 / vintage 2x15",
        kind: Kind::Ir,
        skin: "cab_2x12",
        rs: &["CA215C", "CS1515C", "HG215C"],
        query: "2x15 cab",
        bass: false,
    },
    Unit {
        name: "Ampeg SVT-810E",
        kind: Kind::Ir,
        skin: "cab_8x10",
        rs: &["AT810BC", "OrangeOBC810", "EdenD610XST"],
        query: "Ampeg 8x10",
        bass: true,
    },
    Unit {
        name: "Bass 4x10",
        kind: Kind::Ir,
        skin: "cab_8x10",
        rs: &["BT410BC", "CH410BC", "CS410BC", "EdenD410XST"],
        query: "Eden 4x10",
        bass: true,
    },
    Unit {
        name: "Bass 2x10 / 2x12",
        kind: Kind::Ir,
        skin: "cab_8x10",
        rs: &["CH210BC", "BT212BC", "EdenD212XLT", "CH310BC"],
        query: "bass 2x10",
        bass: true,
    },
    Unit {
        name: "Ampeg B-15 / bass 1x15",
        kind: Kind::Ir,
        skin: "cab_1x15",
        rs: &[
            "AT1150BC",
            "BT115BC",
            "CA1510BC",
            "CS15BC",
            "CS112BC",
            "EdenD115XLT",
            "OrangeOBC115",
            "GB415BC",
            "TW215BC",
        ],
        query: "Ampeg B-15 cab",
        bass: true,
    },
    Unit {
        name: "Marshall JCM900",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &[],
        query: "Marshall JCM900",
        bass: false,
    },
    Unit {
        name: "Marshall Silver Jubilee",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &[],
        query: "Marshall Silver Jubilee",
        bass: false,
    },
    Unit {
        name: "Marshall JCM2000 DSL/TSL",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &[],
        query: "Marshall JCM2000",
        bass: false,
    },
    Unit {
        name: "Friedman Dirty Shirley / BE",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &[],
        query: "Friedman Dirty Shirley",
        bass: false,
    },
    Unit {
        name: "Mesa/Boogie Mark IV",
        kind: Kind::Nam,
        skin: "amp_lead",
        rs: &[],
        query: "Mesa Boogie Mark IV",
        bass: false,
    },
    Unit {
        name: "EVH 5150 III",
        kind: Kind::Nam,
        skin: "amp_modern",
        rs: &[],
        query: "EVH 5150 III",
        bass: false,
    },
    Unit {
        name: "ENGL Powerball / Fireball",
        kind: Kind::Nam,
        skin: "amp_modern",
        rs: &[],
        query: "ENGL Powerball",
        bass: false,
    },
    Unit {
        name: "Bogner Uberschall",
        kind: Kind::Nam,
        skin: "amp_modern",
        rs: &[],
        query: "Bogner Uberschall",
        bass: false,
    },
    Unit {
        name: "Mesa/Boogie Triple Rectifier",
        kind: Kind::Nam,
        skin: "amp_modern",
        rs: &[],
        query: "Mesa Triple Rectifier",
        bass: false,
    },
    Unit {
        name: "Hiwatt DR103",
        kind: Kind::Nam,
        skin: "amp_crunch",
        rs: &[],
        query: "Hiwatt DR103",
        bass: false,
    },
    Unit {
        name: "Marshall 1987X / Plexi 50W",
        kind: Kind::Nam,
        skin: "amp_crunch",
        rs: &[],
        query: "Marshall 1987",
        bass: false,
    },
    Unit {
        name: "Bad Cat / Dr. Z",
        kind: Kind::Nam,
        skin: "amp_chime",
        rs: &[],
        query: "Dr Z Maz",
        bass: false,
    },
    Unit {
        name: "Fender Super Reverb",
        kind: Kind::Nam,
        skin: "amp_clean",
        rs: &[],
        query: "Fender Super Reverb",
        bass: false,
    },
    Unit {
        name: "Fender Bassman silverface (guitar)",
        kind: Kind::Nam,
        skin: "amp_clean",
        rs: &[],
        query: "Fender Silverface Bassman",
        bass: false,
    },
    Unit {
        name: "Fender Blues Junior",
        kind: Kind::Nam,
        skin: "amp_clean",
        rs: &[],
        query: "Fender Blues Junior",
        bass: false,
    },
    Unit {
        name: "Dumble Overdrive Special",
        kind: Kind::Nam,
        skin: "amp_clean",
        rs: &[],
        query: "Dumble ODS",
        bass: false,
    },
    Unit {
        name: "Fender Champ 5F1",
        kind: Kind::Nam,
        skin: "amp_tweed",
        rs: &[],
        query: "Fender Champ 5F1",
        bass: false,
    },
    Unit {
        name: "Fender Tweed Twin / Pro",
        kind: Kind::Nam,
        skin: "amp_tweed",
        rs: &[],
        query: "Fender Tweed Twin",
        bass: false,
    },
    Unit {
        name: "Aguilar Tone Hammer / DB750",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &["Bass_Amp_BT975B"],
        query: "Aguilar",
        bass: true,
    },
    Unit {
        name: "Trace Elliot",
        kind: Kind::Nam,
        skin: "amp_bass",
        rs: &[],
        query: "Trace Elliot",
        bass: true,
    },
    Unit {
        name: "Ampeg SCR-DI",
        kind: Kind::Pedal,
        skin: "bass_drive",
        rs: &["DI_Amp_TubePre"],
        query: "Ampeg SCR-DI",
        bass: true,
    },
    Unit {
        name: "Tech 21 SansAmp Bass Driver",
        kind: Kind::Pedal,
        skin: "bass_drive",
        rs: &["DI_Amp_BassDriver"],
        query: "SansAmp Bass Driver",
        bass: true,
    },
    Unit {
        name: "Klon Centaur (original)",
        kind: Kind::Pedal,
        skin: "gold",
        rs: &["Pedal_LineDrive"],
        query: "Klon Centaur",
        bass: false,
    },
    Unit {
        name: "Analogman King of Tone",
        kind: Kind::Pedal,
        skin: "germanium",
        rs: &[],
        query: "King of Tone",
        bass: false,
    },
    Unit {
        name: "Marshall Bluesbreaker pedal",
        kind: Kind::Pedal,
        skin: "germanium",
        rs: &[],
        query: "Marshall Bluesbreaker pedal",
        bass: false,
    },
    Unit {
        name: "Fuzz Face (silicon)",
        kind: Kind::Pedal,
        skin: "round_fuzz",
        rs: &["Pedal_CaptFuzzle"],
        query: "Fuzz Face silicon",
        bass: false,
    },
    Unit {
        name: "Boss DS-2 / Turbo Distortion",
        kind: Kind::Pedal,
        skin: "orange",
        rs: &[],
        query: "Boss DS-2",
        bass: false,
    },
    Unit {
        name: "Electro-Harmonix Big Muff Pi (Russian)",
        kind: Kind::Pedal,
        skin: "big_pi",
        rs: &["Pedal_BuzzToo", "Pedal_FuzzWasHe"],
        query: "Sovtek Big Muff",
        bass: false,
    },
    Unit {
        name: "Boss AC-3 Acoustic Simulator",
        kind: Kind::Pedal,
        skin: "acoustic",
        rs: &["Pedal_AcousticEmulator"],
        query: "Boss AC-3",
        bass: false,
    },
    Unit {
        name: "Marshall 1960AV 4x12 (V30)",
        kind: Kind::Ir,
        skin: "cab_4x12",
        rs: &[],
        query: "Marshall 1960AV",
        bass: false,
    },
    Unit {
        name: "EVH 5150 III 4x12",
        kind: Kind::Ir,
        skin: "cab_4x12",
        rs: &[],
        query: "EVH 5150 4x12",
        bass: false,
    },
    Unit {
        name: "Bogner / Friedman closed cab",
        kind: Kind::Ir,
        skin: "cab_4x12",
        rs: &[],
        query: "Friedman 4x12",
        bass: false,
    },
    Unit {
        name: "Matchless / Bogner 2x12",
        kind: Kind::Ir,
        skin: "cab_2x12",
        rs: &[],
        query: "Matchless 2x12",
        bass: false,
    },
    Unit {
        name: "Mesa 2x12",
        kind: Kind::Ir,
        skin: "cab_2x12",
        rs: &[],
        query: "Mesa 2x12",
        bass: false,
    },
    Unit {
        name: "Vox AC15 1x12",
        kind: Kind::Ir,
        skin: "cab_1x12",
        rs: &[],
        query: "Vox AC15 cab",
        bass: false,
    },
    Unit {
        name: "Mesa 1x12",
        kind: Kind::Ir,
        skin: "cab_1x12",
        rs: &["CA112C"],
        query: "Mesa 1x12",
        bass: false,
    },
    Unit {
        name: "Fender Champ / tweed 1x8-1x10",
        kind: Kind::Ir,
        skin: "cab_1x10",
        rs: &["TW110C"],
        query: "Fender Champ cab",
        bass: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn song(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    #[test]
    fn units_are_ranked_by_the_songs_that_use_them() {
        let songs = vec![
            song(&["Amp_AT20", "Cab_TW110C_57_Cone"]),
            song(&["Amp_AT20"]),
            song(&["Amp_TW26"]),
            song(&["Amp_Nothing"]),
        ];
        let (list, covered) = rank(&songs, |_| false, 10, false);
        assert_eq!(list[0].unit.name, "Fender Deluxe Reverb");
        assert_eq!(list[0].songs, 2);
        // the Twin and the Princeton share Amp_TW26: listed once
        assert_eq!(list.iter().filter(|s| s.unit.rs == ["Amp_TW26"]).count(), 1);
        // a cab by its body
        assert!(list.iter().any(|s| s.unit.kind == Kind::Ir && s.songs == 1));
        assert_eq!(covered, 3);
        let (top, covered) = rank(&songs, |u| u.rs.contains(&"Amp_AT20"), 1, false);
        assert_eq!(top.len(), 1);
        assert!(top[0].have);
        assert_eq!(covered, 2);
        assert!(rank(&[], |_| false, 5, false).0.is_empty());
        assert_eq!(
            search_url("Fender Hot-Rod Deville"),
            "https://www.tone3000.com/search?q=Fender+Hot+Rod+Deville"
        );
    }

    /// P37.6a: a bass player's list is bass gear only, led by the heads their bass parts use most; a guitarist's
    /// never has a bass head, even when the bass parts use one in every song.
    #[test]
    fn a_bass_ranks_only_bass_gear() {
        let bass_parts = vec![
            song(&["Bass_Amp_HT300B", "Bass_Cab_AT810BC_57_Cone", "Bass_Pedal_MBComp"]),
            song(&["Bass_Amp_HT300B", "DI_Amp_BassDriver"]),
            song(&["Bass_Amp_LT25B", "Bass_Cab_AT1150BC_Tube_Cone"]),
            song(&["Bass_Amp_HT300B", "DI_Amp_TubePre"]),
        ];
        let (list, covered) = rank(&bass_parts, |_| false, 10, true);
        assert!(list.iter().all(|s| s.unit.bass), "{list:?}");
        assert_eq!(list[0].unit.name, "Ampeg SVT");
        assert_eq!(list[0].songs, 3);
        let names: Vec<&str> = list.iter().map(|s| s.unit.name).collect();
        for n in [
            "Ampeg B-15 Portaflex",
            "Tech 21 SansAmp Bass Driver",
            "Ampeg SVT-810E",
            "Ampeg SCR-DI",
        ] {
            assert!(names.contains(&n), "{n} in {names:?}");
        }
        assert_eq!(covered, 4);
        // a guitarist's list from guitar parts that share a song with those bass parts: no bass gear
        let guitar_parts = vec![song(&["Amp_AT20", "Bass_Amp_HT300B"]), song(&["Amp_TW26"])];
        let (list, _) = rank(&guitar_parts, |_| false, 10, false);
        assert!(list.iter().all(|s| !s.unit.bass), "{list:?}");
        assert_eq!(list.len(), 2);
    }

    /// P37.6a (the tinkerer's "Princeton 1x10 IR on 40 of 40 songs"): a unit is counted only for the songs whose
    /// own gear names it; a cab no song names is never suggested, and a unit's count can't pass the songs that
    /// name it.
    #[test]
    fn a_cab_counts_only_the_songs_that_use_it() {
        let songs = vec![
            song(&["Amp_AT20", "Cab_TW110C_57_Cone"]),
            song(&["Amp_AT20", "Cab_EN212C_57_Cone"]),
            song(&["Amp_CA100", "Cab_CA412C_57_Cone"]),
            song(&["Amp_CA100", "Cab_CA412C_57_Cone"]),
        ];
        let (list, _) = rank(&songs, |_| false, 20, false);
        let count = |name: &str| list.iter().find(|s| s.unit.name == name).map(|s| s.songs);
        assert_eq!(count("Fender Princeton 1x10"), Some(1));
        assert_eq!(count("Mesa Rectifier 4x12 (V30)"), Some(2));
        assert_eq!(count("Marshall 1960AV 4x12 (V30)"), None);
        assert!(list.iter().all(|s| s.songs <= songs.len()));
    }
}
