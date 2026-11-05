//! An RS tone as a board: each piece of gear in the song's rig (qrs-formats `rig`) becomes the model that
//! stands in for it, with its knobs carried over in their own units. What each piece of gear is and its knobs'
//! units come from the game's own gear manifests and descriptions (gears.psarc `manifests/gears/*.json`,
//! static.psarc `localization/maingame.csv`; D43): RS's amps with code names are mapped by what they are
//! ("EN-30, a UK-style mid-gain amp" with Top Boost = a Vox AC30), and only an amp this table doesn't know falls
//! back to the tone's descriptors and Gain. Cabs are mapped by their speakers (and open or closed back) and the
//! mic by the key's ending (`_57_Edge`, `_Ribbon_Cone`, `_Tube_OffAxis`). Knob units: "number" knobs are
//! 1..100 (our dials 0..10), "%" stays %, "db" stays dB (a dB tone stack becomes a dial at 5 + dB * 5/12), Hz
//! rates go to our dials through each model's own rate curve, enums map to their positions. Gear with no
//! counterpart here (ring mod, harmonies other than octaves) is left out; a PA cab or a DI preamp is flat on
//! purpose (no block).

use qrs_formats::rig::{Gear, RigTone};

use super::{Block, Board, MAX_BLOCKS};

/// An RS 0-100 knob as a 0-10 dial.
fn dial(g: &Gear, name: &str) -> Option<f32> {
    g.knob(name).map(|v| (v / 10.0).clamp(0.0, 10.0))
}

/// An RS time knob as milliseconds (some old tones store seconds).
fn millis(g: &Gear, name: &str) -> Option<f32> {
    g.knob(name).map(|v| if v <= 5.0 { v * 1000.0 } else { v })
}

/// A dB tone control (-12..12) as a dial: flat is 5.
fn db_dial(db: f32) -> f32 {
    (5.0 + db * 5.0 / 12.0).clamp(0.0, 10.0)
}

/// A value on one of our log sweeps (`dsp::sweep(lo, hi, dial / 10)`) as the dial that gives it; `lo` may be
/// above `hi` (a sweep that falls as the dial turns up).
pub(crate) fn sweep_dial(v: f32, lo: f32, hi: f32) -> f32 {
    if v <= 0.0 {
        return if lo < hi { 0.0 } else { 10.0 };
    }
    (10.0 * (v / lo).ln() / (hi / lo).ln()).clamp(0.0, 10.0)
}

fn set_dial(b: &mut Block, key: &str, g: &Gear, name: &str) {
    if let Some(v) = dial(g, name) {
        b.set(key, v);
    }
}

fn set_raw(b: &mut Block, key: &str, g: &Gear, name: &str) {
    if let Some(v) = g.knob(name) {
        b.set(key, v);
    }
}

/// An RS rate in Hz onto a model's rate dial (`lo`..`hi` Hz is the model's sweep).
fn set_hz(b: &mut Block, key: &str, g: &Gear, name: &str, lo: f32, hi: f32) {
    if let Some(v) = g.knob(name) {
        b.set(key, sweep_dial(v, lo, hi));
    }
}

/// A kHz knob (RS's studio EQs) as Hz; old tones may already hold Hz.
fn khz(v: f32) -> f32 {
    if v < 30.0 { v * 1000.0 } else { v }
}

/// One of RS's code-named (or named) amps: what it is, and the voicing that stands in for it.
struct AmpId {
    key: &'static str,
    /// What it is (the game's description, and the real amp it is modelled on where that is clear).
    real: &'static str,
    voicing: &'static str,
    /// Below this gain (0..10) the amp is played as a clean amp: a channel amp's clean channel (0: never).
    clean_below: f32,
}

const fn a(key: &'static str, real: &'static str, voicing: &'static str, clean_below: f32) -> AmpId {
    AmpId {
        key,
        real,
        voicing,
        clean_below,
    }
}

/// RS2014's guitar amps. Sources: the game's descriptions (maingame.csv), the knobs and the faceplates
/// (gears.psarc), and CustomsForge's "Tone Designer to real-world gear" list (D43).
const AMPS: &[AmpId] = &[
    a(
        "Amp_AT20",
        "AT-20: vintage US low-gain (a Fender Deluxe Reverb)",
        "amp_clean",
        0.0,
    ),
    a("Amp_AT120", "AT-120: vintage US high-gain", "amp_lead", 2.5),
    a(
        "Amp_BT15",
        "BTQ-15: low-gain boutique (Matchless-style class A)",
        "amp_chime",
        0.0,
    ),
    a(
        "Amp_BT30",
        "BTQ-30: mid-gain boutique (Matchless DC-30-style)",
        "amp_chime",
        0.0,
    ),
    a(
        "Amp_BT45",
        "BTQ-45: high-gain boutique (Bogner/Friedman-style)",
        "amp_snake",
        2.5,
    ),
    a("Amp_CA38", "Cali-38: Mesa/Boogie Mark I ('70s)", "amp_cali", 4.0),
    a(
        "Amp_CA85",
        "Cali-85: Mesa/Boogie Mark IIC+/III ('80s)",
        "amp_cali",
        3.0,
    ),
    a(
        "Amp_CA100",
        "Cali-100: Mesa/Boogie Dual Rectifier ('90s)",
        "amp_modern",
        2.5,
    ),
    a(
        "Amp_CS90",
        "CL-SS 90: jazz solid state (Roland JC-120)",
        "amp_clean",
        0.0,
    ),
    a(
        "Amp_CS100",
        "CL-SS 100: Polytone-style solid state",
        "amp_clean",
        0.0,
    ),
    a("Amp_CS120", "CL-SS 120: high-watt solid state", "amp_clean", 0.0),
    a("Amp_EN30", "EN-30: Vox AC30 Top Boost", "amp_chime", 0.0),
    a(
        "Amp_EN50",
        "EN-50: UK-style high-gain (Vox AC50-style)",
        "amp_chime",
        0.0,
    ),
    a("Amp_GB38", "GBC-38: low-gain British crunch", "amp_crunch", 0.0),
    a("Amp_GB50", "GBC-50: mid-gain British crunch", "amp_crunch", 0.0),
    a(
        "Amp_GB100",
        "GBC-1H: high-gain British crunch (Laney AOR-style)",
        "amp_lead",
        0.0,
    ),
    a(
        "Amp_HG100",
        "HG-100: Soldano SLO-100 (the snakeskin front)",
        "amp_snake",
        2.5,
    ),
    a("Amp_HG180", "HG-180: boutique high-gain", "amp_modern", 2.5),
    a(
        "Amp_HG500",
        "HG-500: ultra high-gain with active EQ (Peavey 5150-style)",
        "amp_snake",
        2.5,
    ),
    a("Amp_TW22", "AMTW-22: Fender tweed (5E3 Deluxe)", "amp_tweed", 0.0),
    a(
        "Amp_TW26",
        "AMTW-26: Fender blackface/silverface combo",
        "amp_clean",
        0.0,
    ),
    a(
        "Amp_TW40",
        "AMTW-40: Fender tweed Bassman (4x10)",
        "amp_tweed",
        0.0,
    ),
    a(
        "Amp_EpiphoneElectarCentury",
        "Epiphone Electar Century (1939, 4 W)",
        "amp_tweed",
        0.0,
    ),
    a(
        "Amp_EpiphoneElectarM",
        "Epiphone Electar Model M (1936, 15 W)",
        "amp_tweed",
        0.0,
    ),
    a(
        "Amp_EpiphoneElectricHawaiian",
        "Epiphone Electric Hawaiian (1935, 10 W)",
        "amp_tweed",
        0.0,
    ),
    a(
        "Amp_EpiphoneZephyr",
        "Epiphone Zephyr (1955, 30 W)",
        "amp_tweed",
        0.0,
    ),
    a(
        "Amp_GibsonGA8",
        "Gibson GA-8 Discoverer (1962, 15 W)",
        "amp_tweed",
        0.0,
    ),
    a("Amp_GibsonGA79", "Gibson GA-79 (30 W)", "amp_tweed", 0.0),
    a("Amp_GibsonGA88", "Gibson GA-88 (34 W head)", "amp_tweed", 0.0),
    a(
        "Amp_MarshallPlexi",
        "Marshall Super Lead (Plexi)",
        "amp_crunch",
        0.0,
    ),
    a("Amp_MarshallJTM45", "Marshall JTM45", "amp_crunch", 0.0),
    a(
        "Amp_Marshall1962Bluesbreaker",
        "Marshall 1962 Bluesbreaker",
        "amp_crunch",
        0.0,
    ),
    a("Amp_MarshallJCM800", "Marshall JCM800", "amp_lead", 0.0),
    a("Amp_MarshallDSL15H", "Marshall DSL15H", "amp_lead", 2.5),
    a("Amp_MarshallDSL100H", "Marshall DSL100H", "amp_lead", 2.5),
    a("Amp_MarshallJVM410H", "Marshall JVM410H", "amp_lead", 2.5),
    a("Amp_OrangeAD50", "Orange AD50", "amp_tangerine", 0.0),
    a("Amp_OrangeOR50H", "Orange OR50H", "amp_tangerine", 0.0),
    a("Amp_OrangeOR100", "Orange OR100", "amp_tangerine", 0.0),
    a("Amp_OrangeTinyTerror", "Orange Tiny Terror", "amp_tangerine", 0.0),
    a(
        "Amp_OrangeRockerverb",
        "Orange Rockerverb 50 MkII",
        "amp_tangerine",
        2.5,
    ),
    a(
        "Amp_OrangeJimmyBean",
        "Orange 'Jimmy Bean' (Jim Root's solid state)",
        "amp_modern",
        0.0,
    ),
];

/// Bass amps whose tone stacks are in dB (-12..12) rather than 0-100 knobs.
const DB_STACK: &[&str] = &[
    "Bass_Amp_BT880B",
    "Bass_Amp_BT975B",
    "Bass_Amp_CH300B",
    "Bass_Amp_CH600B",
    "Bass_Amp_CS75B",
    "Bass_Amp_EdenE300",
    "Bass_Amp_EdenWT550",
    "Bass_Amp_EdenWT800",
    "Bass_Amp_HT400B",
];

/// RS2014's bass amps (P37.2): what each is and which of our bass voices plays it. Before, every one was the
/// SVT voice. Sources: the game's descriptions and CustomsForge's list (`soundroom-review/BASS-GEAR.md`); keys we
/// have not seen fall back on the SVT (`bass_voicing`).
const BASS_AMPS: &[AmpId] = &[
    a(
        "Bass_Amp_HT300B",
        "HW-300B: classic tube bass head (an Ampeg SVT-CL)",
        "amp_bass",
        0.0,
    ),
    a(
        "Bass_Amp_HT400B",
        "HW-400B: boutique tube bass head with plenty of drive",
        "amp_bass_tube",
        0.0,
    ),
    a(
        "Bass_Amp_CH300B",
        "CLH-300B: classic tube/transistor hybrid (a Gallien-Krueger 800RB)",
        "amp_bass_ss",
        0.0,
    ),
    a(
        "Bass_Amp_CH600B",
        "CLH-600B: hybrid with graphic EQ and compression (an Ampeg SVT-4 Pro)",
        "amp_bass_ss",
        0.0,
    ),
    a(
        "Bass_Amp_CS75B",
        "CSS-75B: boutique transistor bass head (Hartke / Markbass style)",
        "amp_bass_ss",
        0.0,
    ),
    a(
        "Bass_Amp_BT880B",
        "BH-880B: boutique hybrid with parametric EQ",
        "amp_bass_ss",
        0.0,
    ),
    a(
        "Bass_Amp_BT975B",
        "BH-975B: boutique tube bass head (an Aguilar DB751)",
        "amp_bass_tube",
        0.0,
    ),
    a(
        "Bass_Amp_LT25B",
        "LW-25B: classic low-wattage tube bass (an Ampeg B-15 Portaflex)",
        "amp_b15",
        0.0,
    ),
    // (P38.2: the CustomsForge list's other bass heads, by family: HW tube heads, CLH hybrids, CSS transistor
    // heads, BH boutique hybrids, LW low-wattage tube combos; ~750 of the owner's bass tones were on the SVT voice)
    a(
        "Bass_Amp_HT100B",
        "HW-100B: classic tube bass head (an Ampeg SVT-style head with Hi/Lo shift)",
        "amp_bass",
        0.0,
    ),
    a(
        "Bass_Amp_CH350B",
        "CLH-350B: tube/transistor hybrid with a graphic EQ (Gallien-Krueger style)",
        "amp_bass_ss",
        0.0,
    ),
    a(
        "Bass_Amp_CS240B",
        "CSS-240B: transistor bass head",
        "amp_bass_ss",
        0.0,
    ),
    a(
        "Bass_Amp_CS300B",
        "CSS-300B: transistor bass head with split mids",
        "amp_bass_ss",
        0.0,
    ),
    a(
        "Bass_Amp_BT600B",
        "BH-600B: boutique hybrid with compression and parametric EQ",
        "amp_bass_ss",
        0.0,
    ),
    a(
        "Bass_Amp_LT85B",
        "LW-85B: classic low-wattage tube bass combo (Portaflex style)",
        "amp_b15",
        0.0,
    ),
    a("Bass_Amp_OrangeAD200B", "Orange AD200B", "amp_bass_tube", 0.0),
    a("Bass_Amp_EdenE300", "Eden E300", "amp_bass_ss", 0.0),
    a("Bass_Amp_EdenWT550", "Eden WT550", "amp_bass_ss", 0.0),
    a("Bass_Amp_EdenWT800", "Eden WT800", "amp_bass_ss", 0.0),
];

/// Our bass voice for an RS bass amp key (the SVT for one no table knows, unless its name says Orange).
pub fn bass_voicing(key: &str) -> &'static str {
    match BASS_AMPS.iter().find(|a| a.key == key) {
        Some(a) => a.voicing,
        None if key.to_lowercase().contains("orange") => "amp_bass_tube",
        None => "amp_bass",
    }
}

/// What an RS amp or cab key stands for, when known (for reports).
pub fn real_gear(key: &str) -> Option<&'static str> {
    AMPS.iter()
        .chain(BASS_AMPS)
        .find(|a| a.key == key)
        .map(|a| a.real)
        .or_else(|| cab_id(key).map(|c| c.real))
}

/// Whether this amp is mapped by what it is (true) or guessed from the tone (false).
pub fn amp_known(key: &str) -> bool {
    AMPS.iter().any(|a| a.key == key) || is_bass_amp(key)
}

fn bass_amp_known(key: &str) -> bool {
    key.starts_with("DI_Amp") || BASS_AMPS.iter().any(|a| a.key == key)
}

fn is_bass_amp(key: &str) -> bool {
    key.starts_with("Bass_Amp") || key.starts_with("DI_Amp")
}

/// A tone for a bass: it says so, or it has a bass amp or cab. (Not a bass pedal: RS's acoustic guitar
/// tones often use the bass multiband compressor.)
pub fn is_bass_tone(tone: &RigTone) -> bool {
    if tone.has("BASS") {
        return true;
    }
    if tone.has("ACOUSTIC") {
        return false;
    }
    tone.gear
        .iter()
        .any(|g| g.key.starts_with("Bass_Amp") || g.key.starts_with("Bass_Cab"))
        || tone.key.to_lowercase().contains("bass")
}

/// An amp no table knows: the tone's descriptors and the amp's Gain pick the voicing.
fn guess_amp(key: &str, tone: &RigTone, gain: f32) -> &'static str {
    let k = key.to_lowercase();
    if k.contains("plexi") || k.contains("jtm45") || k.contains("bluesbreaker") {
        return "amp_crunch";
    }
    // (P41.2, D167: the new voices for the gear the reviews named; RS's own amps are in `AMPS`)
    if k.contains("orange") {
        return "amp_tangerine";
    }
    if k.contains("hiwatt") {
        return "amp_surrey";
    }
    if k.contains("soldano") || k.contains("5150") || k.contains("6505") || k.contains("bogner") {
        return "amp_snake";
    }
    if k.contains("boogie") || k.contains("mark") {
        return "amp_cali";
    }
    if k.contains("jcm800") || k.contains("dsl") || k.contains("jvm") {
        return "amp_lead";
    }
    if tone.has("HIGH GAIN") || (tone.has("DISTORTION") && gain >= 6.0) {
        return "amp_modern";
    }
    if tone.has("DISTORTION") {
        return "amp_lead";
    }
    // a "clean lead" is clean
    if tone.has("CLEAN") || tone.has("ACOUSTIC") {
        return "amp_clean";
    }
    if tone.has("OVERDRIVE") || tone.has("FUZZ") {
        return "amp_crunch";
    }
    if tone.has("LEAD") {
        return if gain >= 5.0 { "amp_lead" } else { "amp_crunch" };
    }
    if tone.has("REVERB") || tone.has("CHORUS") || tone.has("ECHO") {
        return "amp_clean";
    }
    match gain {
        g if g < 3.5 => "amp_clean",
        g if g < 6.5 => "amp_crunch",
        _ => "amp_lead",
    }
}

/// Under this gain (0..10) a channel-switching amp plays its clean channel, whatever the tone's descriptors say.
pub const LOW_GAIN: f32 = 1.5;

fn amp(g: &Gear, tone: &RigTone) -> Option<Vec<Block>> {
    if is_bass_amp(&g.key) {
        return bass_amp(g, tone);
    }
    let gain = dial(g, "Gain")
        .or_else(|| {
            let a = dial(g, "Loudness1").unwrap_or(0.0);
            let b = dial(g, "Loudness2").unwrap_or(0.0);
            (a > 0.0 || b > 0.0).then_some(a.max(b))
        })
        .unwrap_or(5.0);
    // a channel amp turned down for a tone that doesn't call itself driven is its clean channel (a Rectifier
    // at Gain 20 is still a crunch, and RS's "distortion" tones on it are)
    let driven = ["OVERDRIVE", "DISTORTION", "HIGH GAIN", "CRUNCH", "FUZZ"]
        .iter()
        .any(|d| tone.has(d));
    // (P41.1, the gigging guitarist's #3: a channel amp nearly off, under `LOW_GAIN`, is its clean channel whatever
    // the tone calls itself: Black Hole Sun's "rotomod", a Mark IIC+ at gain 0.1, is no lead tone)
    let clean_channel = |below: f32| {
        gain < LOW_GAIN || (!driven && (gain < below || (tone.has("CLEAN") && gain < 2.0 * below)))
    };
    let id = match AMPS.iter().find(|a| a.key == g.key) {
        Some(a) if a.clean_below > 0.0 && clean_channel(a.clean_below) => "amp_clean",
        Some(a) => a.voicing,
        None => guess_amp(&g.key, tone, gain),
    };
    // (P41.1: a channel amp's clean channel has its own volume: RS's gain 0.1 on a Mark is its clean channel played
    // at a normal level, not our Twin all but off; at least 3, still clean, so the level match has headroom)
    let gain = if id == "amp_clean" && AMPS.iter().any(|a| a.key == g.key && a.clean_below > 0.0) {
        gain.max(3.0)
    } else {
        gain
    };
    let mut b = Block::of(id)?;
    // a one-knob amp (Tiny Terror, Electric Hawaiian) has a Tone for its treble
    let treble = dial(g, "Treble").or_else(|| dial(g, "Tone"));
    match id {
        "amp_tweed" => {
            b.set("volume", gain);
            if let Some(t) = treble {
                // a tweed's one tone knob: the treble, pulled down a little by a heavy bass setting
                let bass = dial(g, "Bass").unwrap_or(5.0);
                b.set("tone", (t - 0.3 * (bass - 5.0)).clamp(0.0, 10.0));
            }
        }
        "amp_chime" => {
            b.set("gain", gain);
            if let Some(t) = treble {
                b.set("treble", t);
            }
            set_dial(&mut b, "bass", g, "Bass");
            // the AC30's Cut takes the top off where a presence adds it
            if let Some(p) = dial(g, "Pres") {
                b.set("cut", 10.0 - p);
            }
        }
        _ => {
            b.set("gain", gain);
            set_dial(&mut b, "bass", g, "Bass");
            set_dial(&mut b, "mid", g, "Mid");
            if let Some(t) = treble {
                b.set("treble", t);
            }
            set_dial(&mut b, "presence", g, "Pres");
            if let Some(v) = g.knob("Bright") {
                b.set("bright", if v >= 0.5 { 1.0 } else { 0.0 });
            }
        }
    }
    Some(vec![b])
}

/// The nearest of the bass 8-band EQ's bands to a frequency.
const BASS_EQ8: [f32; 8] = [30.0, 75.0, 185.0, 460.0, 1100.0, 2700.0, 6800.0, 16000.0];

fn nearest_band(f: f32) -> usize {
    let mut best = 0;
    for (i, b) in BASS_EQ8.iter().enumerate() {
        if (f / b).ln().abs() < (f / BASS_EQ8[best]).ln().abs() {
            best = i;
        }
    }
    best
}

/// A bass amp (or a DI preamp): the amp with its tone stack in its own units, and what else it has built in (a
/// graphic EQ, a compressor, a parametric EQ) as blocks beside it.
fn bass_amp(g: &Gear, tone: &RigTone) -> Option<Vec<Block>> {
    let k = g.key.as_str();
    if k == "DI_Amp_BassDriver" {
        // the SansAmp Bass Driver DI
        let mut x = Block::of("bass_drive")?;
        set_dial(&mut x, "gain", g, "Gain");
        set_raw(&mut x, "blend", g, "Blend");
        set_dial(&mut x, "tone", g, "Treble");
        return Some(vec![x]);
    }
    if k.starts_with("DI_Amp") && !is_bass_tone(tone) {
        // a guitar (usually an acoustic) straight into a desk preamp: flat
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let db = DB_STACK.contains(&k);
    let stack = |name: &str| -> Option<f32> {
        g.knob(name).map(|v| {
            if db {
                db_dial(v)
            } else {
                (v / 10.0).clamp(0.0, 10.0)
            }
        })
    };
    if let Some(c) = g.knob("Compression").filter(|c| *c > 5.0) {
        let mut x = Block::of("bass_comp")?;
        x.set("compress", c / 10.0);
        out.push(x);
    }
    let voicing = bass_voicing(k);
    let mut b = Block::of(voicing)?;
    let gain = dial(g, "Gain").unwrap_or(4.0);
    b.set(if voicing == "amp_b15" { "volume" } else { "gain" }, gain);
    let bass = stack("Bass");
    let treble = stack("Treble");
    let mid = stack("Mid").or_else(|| match (stack("LoMid"), stack("HiMid")) {
        (Some(l), Some(h)) => Some((l + h) / 2.0),
        (l, h) => l.or(h),
    });
    if voicing == "amp_bass_ss" {
        // the active EQ in dB: a 0..10 dial at 5 is flat, +-15 dB at its ends
        let to_db = |d: Option<f32>| d.map(|d| ((d - 5.0) * 3.0).clamp(-15.0, 15.0));
        for (key, v) in [
            ("bass", to_db(bass)),
            ("mid", to_db(mid)),
            ("treble", to_db(treble)),
        ] {
            if let Some(v) = v {
                b.set(key, v);
            }
        }
    } else {
        for (key, v) in [("bass", bass), ("mid", mid), ("treble", treble)] {
            if let Some(v) = v {
                b.set(key, v);
            }
        }
    }
    // the Ampeg-style Ultra Hi / Ultra Lo switches: the SVT's own (P37.2); on another voice a treble lift, and a
    // bass lift with the mids cut, as before
    let (hi_shift, lo_shift) = (
        g.knob("HiShift").is_some_and(|v| v >= 0.5),
        g.knob("LoShift").is_some_and(|v| v >= 0.5),
    );
    if voicing == "amp_bass" {
        b.set("ultra_hi", if hi_shift { 1.0 } else { 0.0 });
        b.set("ultra_lo", if lo_shift { 1.0 } else { 0.0 });
    } else if voicing != "amp_bass_ss" {
        if hi_shift {
            let t = b.get("treble").unwrap_or(5.0);
            b.set("treble", (t + 1.0).min(10.0));
        }
        if lo_shift {
            let (bb, mm) = (b.get("bass").unwrap_or(5.0), b.get("mid").unwrap_or(5.0));
            b.set("bass", (bb + 1.0).min(10.0));
            b.set("mid", (mm - 1.0).max(0.0));
        }
    } else {
        if hi_shift {
            let t = b.get("treble").unwrap_or(0.0);
            b.set("treble", t + 3.0);
        }
        if lo_shift {
            b.set("contour", 1.0);
        }
    }
    if let Some(f) = g.knob("MidFreq") {
        let hz = if f <= 4.5 {
            // a switch: the SVT's three positions, the SVT-CL's five
            let three = [220.0, 800.0, 3000.0];
            let five = [220.0, 450.0, 800.0, 1600.0, 3000.0];
            let i = f.round().max(0.0) as usize;
            if k == "Bass_Amp_HT300B" {
                five[i.min(4)]
            } else {
                three[i.min(2)]
            }
        } else {
            khz(f)
        };
        b.set("freq", hz);
    } else if g.knob("LoMid").is_some() {
        b.set("freq", 800.0);
    }
    out.push(b);
    // a graphic EQ built into the amp
    let mut bands = [0.0f32; 8];
    let mut any = false;
    for (name, v) in &g.knobs {
        if let Ok(f) = name.parse::<f32>()
            && f >= 20.0
            && v.abs() > 0.01
        {
            let i = nearest_band(f);
            bands[i] = (bands[i] + v).clamp(-15.0, 15.0);
            any = true;
        }
    }
    if any {
        let mut x = Block::of("bass_eq8")?;
        for (i, v) in bands.iter().enumerate() {
            x.set(&format!("b{}", BASS_EQ8[i] as u32), *v);
        }
        out.push(x);
    }
    // a parametric low and high band (the Edens, BH-880B)
    let lo = g.knob("Lo").filter(|v| v.abs() > 0.01);
    let hi = g.knob("Hi").filter(|v| v.abs() > 0.01);
    if lo.is_some() || hi.is_some() {
        let mut x = Block::of("studio_eq")?;
        if let Some(v) = lo {
            x.set("bass", v);
            set_raw(&mut x, "bass_freq", g, "LoFreq");
        }
        if let Some(v) = hi {
            x.set("hi_mid", v);
            if let Some(f) = g.knob("HiFreq") {
                x.set("hi_freq", khz(f));
            }
        }
        out.push(x);
    }
    Some(out)
}

/// One of RS's cabs: its speakers as our model, and what it is.
struct CabId {
    body: &'static str,
    model: &'static str,
    real: &'static str,
}

const fn c(body: &'static str, model: &'static str, real: &'static str) -> CabId {
    CabId { body, model, real }
}

/// By the key's model part (`Cab_<body>_<mic>_<position>`). A 4x10 guitar cab has its own model (P41.2, the
/// Bassman / Super Reverb 4x10); a closed-back modern 2x12 or 2x15 is closer to the 4x12.
const CABS: &[CabId] = &[
    c("AT0112C", "cab_1x12", "AT-0112C: US open-back 1x12"),
    c("AT1121C", "cab_1x12", "AT-112.1C: US open-back 1x12"),
    c("BT1120C", "cab_1x12", "BT-1120C: boutique open-back 1x12"),
    c("BT1121C", "cab_1x12", "BT-112.1C: boutique closed-back 1x12"),
    c("BT410C", "cab_4x10_open", "BT-410C: boutique 4x10"),
    c("CA112C", "cab_1x12", "CA-112C: Mesa-style open-back 1x12"),
    c("CA215C", "cab_2x12", "CA-215C: Mesa-style 2x15"),
    c("CA412C", "cab_4x12", "CA-412C: Mesa-style 4x12"),
    c("CS1120C", "cab_1x12", "CSS-1120C: jazz closed-back 1x12"),
    c("CS1515C", "cab_2x12", "CSS-1515C: vintage 2x15"),
    c("CS212C", "cab_2x12", "CSS-212C: jazz 2x12 (JC-120)"),
    c("EN212C", "cab_2x12", "EN-212C: Vox AC30-style 2x12"),
    c("EN4120C", "cab_4x12", "EN-4120C: UK-style 4x12"),
    c("GB412CMKI", "cab_4x12", "GB-412 MKI: vintage UK 4x12"),
    c("GB412CMKII", "cab_4x12", "GB-412 MKII: UK 4x12"),
    c("GB412CMKIII", "cab_4x12", "GB-412 MKIII: UK 4x12"),
    c("HG2120C", "cab_2x12", "HG-2120C: half-open 2x12"),
    c("HG212C", "cab_4x12", "HG-212C: modern closed-back 2x12"),
    c("HG215C", "cab_4x12", "HG-215C: modern ported 2x15"),
    c("Marshall1936", "cab_4x12", "Marshall 1936 closed-back 2x12"),
    c("Marshall1960a", "cab_4x12", "Marshall 1960A 4x12"),
    c("Marshall1960AX", "cab_4x12", "Marshall 1960AX 4x12"),
    c("Marshall1960TV", "cab_4x12", "Marshall 1960TV 4x12"),
    c("OrangeJimmyBean", "cab_4x12", "Orange 'Jimmy Bean' 4x12"),
    c("OrangePPC212OB", "cab_2x12", "Orange PPC212OB 2x12"),
    c("OrangePPC412", "cab_4x12", "Orange PPC412 4x12"),
    c("TW110C", "cab_1x10", "TW-110C: Fender tweed 1x10"),
    c("TW112C", "cab_1x12", "TW-112C: Fender silverface 1x12"),
    c(
        "TW410C",
        "cab_4x10_open",
        "TW-410C: Fender Bassman 4x10 (open back)",
    ),
    // bass
    c(
        "AT1150BC",
        "cab_1x15",
        "AT-1150BC: closed 1x15 (Ampeg B-15-style)",
    ),
    c("AT810BC", "cab_8x10", "AT-810BC: 8x10 (Ampeg SVT-810)"),
    c("BT115BC", "cab_1x15", "BH-115BC: boutique ported 1x15"),
    c("BT212BC", "cab_2x10", "BH-212BC: boutique ported 2x12"),
    c("BT410BC", "cab_4x10", "BH-410BC: boutique 4x10"),
    c("CA1510BC", "cab_1x15", "CA-1510BC: 1x15 + 10 multi-speaker"),
    c("CH210BC", "cab_2x10", "CH-21BC: ported 2x10"),
    c("CH310BC", "cab_8x10", "CH-3120BC: folded 3x12"),
    c("CH410BC", "cab_4x10", "CH-410BC: 4x10 alloy cones"),
    c("CS112BC", "cab_1x15", "CS-112BC: 1x12"),
    c("CS15BC", "cab_1x15", "CS-15BC: folded 1x15"),
    c("CS410BC", "cab_4x10", "CS-410BC: punchy 4x10"),
    c("EdenD115XLT", "cab_1x15", "Eden D115XLT 1x15"),
    c("EdenD212XLT", "cab_2x10", "Eden D212XLT 2x12"),
    c("EdenD410XST", "cab_4x10", "Eden D410XST 4x10"),
    c("EdenD610XST", "cab_8x10", "Eden D610XST 6x10"),
    c("GB415BC", "cab_4x15", "GB-415BC: vintage 4x15"),
    c("OrangeOBC115", "cab_1x15", "Orange OBC115 1x15"),
    c("OrangeOBC810", "cab_8x10", "Orange OBC810 8x10"),
    c("TW215BC", "cab_4x15", "TW-215BC: closed 2x15"),
    // full-range: no speaker colour (a PA system, hi-fi speakers)
    c("PA1152C", "", "PS-115.2C: small PA"),
    c("PA600C", "", "PS-600: PA"),
    c("PA999C", "", "PA-999C: line-array PA"),
    c("Audiophile", "", "audiophile speakers"),
    c("VintageHiFi", "", "vintage hi-fi"),
    // small, boxy speakers: the lo-fi radio
    c("BoomBox", "lofi", "a boom box"),
    c("CabinetRadio", "lofi", "a console radio"),
    c("Gramophone", "lofi", "a gramophone"),
    c("JukeBox", "lofi", "a jukebox"),
];

/// "Cab_GB412CMKIII_Ribbon_Cone" → "GB412CMKIII".
/// A cab key's model part ("Cab_GB412CMKII_57_Cap" -> "GB412CMKII").
pub fn cab_body(key: &str) -> &str {
    let rest = key
        .strip_prefix("Bass_Cab_")
        .or_else(|| key.strip_prefix("Cab_"))
        .unwrap_or(key);
    rest.split('_').next().unwrap_or(rest)
}

fn cab_id(key: &str) -> Option<&'static CabId> {
    if !key.contains("Cab_") {
        return None;
    }
    let body = cab_body(key);
    CABS.iter().find(|c| c.body.eq_ignore_ascii_case(body))
}

/// A cab no table knows: its speaker layout from the digits in its name.
fn guess_cab(key: &str) -> &'static str {
    let k = key.to_lowercase();
    let bass = k.starts_with("bass_cab");
    let digits: String = cab_body(key).chars().filter(|c| c.is_ascii_digit()).collect();
    let layout: String = digits.chars().take(3).collect();
    if bass {
        return match layout.as_str() {
            "410" => "cab_4x10",
            "210" | "212" => "cab_2x10",
            "415" | "215" => "cab_4x15",
            l if l.ends_with("15") || l.starts_with("15") => "cab_1x15",
            _ => "cab_8x10",
        };
    }
    match layout.as_str() {
        "110" => "cab_1x10",
        "112" => "cab_1x12",
        "410" => "cab_4x10_open",
        l if l.starts_with('4') => "cab_4x12",
        l if l.starts_with('1') => "cab_1x12",
        _ => "cab_2x12",
    }
}

/// "Cab_GB412CMKIII_Ribbon_Cone" → the cab, its mic type and where the mic is.
fn cab(g: &Gear) -> Option<Vec<Block>> {
    let model = match cab_id(&g.key) {
        Some(c) => c.model,
        None => guess_cab(&g.key),
    };
    if model.is_empty() {
        return Some(Vec::new());
    }
    if model == "lofi" {
        return Some(vec![Block::of("lofi")?.with("type", 0.0).with("mix", 100.0)]);
    }
    let k = g.key.to_lowercase();
    let mut b = Block::of(model)?;
    // a "Tube" mic is a valve condenser
    let mic_type = if k.contains("_ribbon") {
        1.0
    } else if k.contains("_condenser") || k.contains("_tube") {
        2.0
    } else {
        0.0
    };
    b.set("mic_type", mic_type);
    if k.ends_with("offaxis") {
        // angled off the cone: darker, a little further back
        b.set("mic", 1.0).set("distance", 3.5);
    } else {
        b.set("mic", if k.ends_with("cone") { 7.0 } else { 2.5 });
    }
    Some(vec![b])
}

/// A pitch knob in semitones as the octaver's voice: +12 (or +24) up, -12 (or -24) down; None for any other
/// interval (a harmony: no counterpart here).
fn octave_of(semitones: f32) -> Option<bool> {
    let s = semitones.round() as i32;
    match s {
        12 | 24 => Some(true),
        -12 | -24 => Some(false),
        _ => None,
    }
}

/// The board model for a pedal or rack, its knobs carried over.
fn effect(g: &Gear) -> Option<Vec<Block>> {
    let k = g.key.as_str();
    let b = |id: &str| Block::of(id);
    let mut blk = match k {
        "Pedal_EQ5" => {
            let mut x = b("eq5")?;
            for f in ["63", "250", "750", "2200", "5700"] {
                set_raw(&mut x, &format!("b{f}"), g, f);
            }
            x
        }
        "Pedal_EQ8" => {
            let mut x = b("eq8")?;
            for f in ["50", "100", "200", "400", "800", "1600", "3200", "6400"] {
                set_raw(&mut x, &format!("b{f}"), g, f);
            }
            x
        }
        "Bass_Pedal_BassEQ8" => {
            let mut x = b("bass_eq8")?;
            for f in ["30", "75", "185", "460", "1100", "2700", "6800", "16000"] {
                set_raw(&mut x, &format!("b{f}"), g, f);
            }
            x
        }
        "Rack_StudioEQ" | "Rack_StudioGraphicEQ" => {
            let mut x = b("studio_eq")?;
            set_raw(&mut x, "bass", g, "Bass");
            set_raw(&mut x, "bass_freq", g, "BassFreq");
            set_raw(&mut x, "lo_mid", g, "LoMid");
            set_raw(&mut x, "lo_freq", g, "LoMidFreq");
            set_raw(&mut x, "hi_mid", g, "HiMid");
            set_raw(&mut x, "treble", g, "Treble");
            // these are in kHz
            if let Some(f) = g.knob("HiMidFreq") {
                x.set("hi_freq", khz(f));
            }
            if let Some(f) = g.knob("TrebleFreq") {
                x.set("treble_freq", khz(f));
            }
            // the paragraphic's fifth band (Mid, in kHz) takes whichever of the two mid bands is flat
            if let Some(m) = g.knob("Mid").filter(|m| m.abs() > 0.01) {
                let f = khz(g.knob("MidFreq").unwrap_or(0.6));
                if g.knob("LoMid").unwrap_or(0.0).abs() < 0.01 {
                    x.set("lo_mid", m).set("lo_freq", f);
                } else if g.knob("HiMid").unwrap_or(0.0).abs() < 0.01 {
                    x.set("hi_mid", m).set("hi_freq", f);
                }
            }
            x
        }
        "Pedal_AmpEQ" => {
            // an amp-style three-knob EQ (0-100, flat at 50) with switched frequencies
            let mut x = b("studio_eq")?;
            let cut = |v: f32| (v - 50.0) * 0.24;
            if let Some(v) = g.knob("Bass") {
                x.set("bass", cut(v));
            }
            if let Some(v) = g.knob("Mid") {
                x.set("lo_mid", cut(v));
            }
            if let Some(v) = g.knob("Treble") {
                x.set("treble", cut(v));
            }
            let pick = |n: &str, opts: &[f32]| {
                let i = g.knob(n).unwrap_or(0.0).round().max(0.0) as usize;
                opts[i.min(opts.len() - 1)]
            };
            x.set("bass_freq", pick("BassFreq", &[80.0, 160.0]));
            x.set("lo_freq", pick("MidShift", &[400.0, 800.0, 1600.0]));
            x.set("treble_freq", pick("TrebleFreq", &[2500.0, 5000.0]));
            x
        }
        "Bass_Pedal_EdenWTDI" => {
            // a bass preamp pedal: its dB tone stack as an EQ
            let mut x = b("studio_eq")?;
            set_raw(&mut x, "bass", g, "Bass");
            x.set("bass_freq", 80.0);
            set_raw(&mut x, "lo_mid", g, "Mid");
            x.set(
                "lo_freq",
                if g.knob("MidShift").is_some_and(|v| v >= 0.5) {
                    1500.0
                } else {
                    500.0
                },
            );
            set_raw(&mut x, "treble", g, "Treble");
            x
        }
        "Bass_Pedal_MBComp" => {
            let mut x = b("bass_comp")?;
            set_dial(&mut x, "compress", g, "Compress");
            set_raw(&mut x, "crossover", g, "Filter");
            set_dial(&mut x, "speed", g, "Rate");
            x
        }
        "Pedal_Compression" => {
            let mut x = b("squeeze")?;
            set_dial(&mut x, "sustain", g, "Comp");
            // its attack in ms onto the Squeeze's attack curve (0.5 + 24.5 * a^2 ms)
            if let Some(ms) = g.knob("Attack") {
                x.set("attack", 10.0 * ((ms - 0.5).max(0.0) / 24.5).sqrt());
            }
            x
        }
        "Pedal_Swole" => {
            // "a very heavy saturated compression"
            let mut x = b("squeeze")?;
            set_dial(&mut x, "sustain", g, "Smash");
            x.set("attack", 2.0);
            x
        }
        "Rack_StudioCompressor" => {
            let mut x = b("studio_comp")?;
            set_raw(&mut x, "threshold", g, "Threshold");
            // a switch: 1.5:1, 2:1, 4:1, 8:1, 20:1
            if let Some(r) = g.knob("Ratio") {
                let ratios = [1.5, 2.0, 4.0, 8.0, 20.0];
                x.set("ratio", ratios[(r.round().max(0.0) as usize).min(4)]);
            }
            set_raw(&mut x, "attack", g, "Attack");
            set_raw(&mut x, "release", g, "Release");
            x
        }
        "Pedal_Limiter" => {
            let limit = g.knob("Limit").unwrap_or(25.0);
            b("studio_comp")?
                .with("ratio", 20.0)
                .with("threshold", -3.0 - 0.3 * limit)
                .with("attack", 1.0)
        }
        "Pedal_NoiseGate" => {
            // (P41.1: RS's own gate is a full one; it moves after the amp, keyed on the input, in `board_keyed`)
            let mut x = b("gate")?;
            x.set("range", -60.0);
            set_raw(&mut x, "threshold", g, "Thresh");
            // the higher the rate, the faster it closes
            if let Some(r) = g.knob("Rate") {
                x.set(
                    "release",
                    500.0 * (10.0f32 / 500.0).powf((r / 100.0).clamp(0.0, 1.0)),
                );
            }
            x
        }
        "Pedal_SpringReverb" | "Pedal_TubeSpring" => {
            let mut x = b("spring")?;
            // the spring's decay (Time) is the dwell; the tube spring has only a Depth
            if g.knob("Time").is_some() {
                set_dial(&mut x, "dwell", g, "Time");
            } else {
                set_dial(&mut x, "dwell", g, "Depth");
            }
            set_raw(&mut x, "mix", g, "Mix");
            x
        }
        "Pedal_PlateVerb" | "Rack_StudioPlate" => {
            let mut x = b("plate")?;
            if let Some(t) = g.knob("Time") {
                x.set("time", 0.5 + t / 100.0 * 5.5);
            }
            set_dial(&mut x, "tone", g, "Tone");
            set_raw(&mut x, "mix", g, "Mix");
            x
        }
        "Pedal_DigitalVerb" | "Rack_StudioVerb" => {
            let mut x = b("hall")?;
            if let Some(t) = g.knob("Time") {
                x.set("time", 1.0 + t / 100.0 * 7.0);
            }
            set_dial(&mut x, "tone", g, "Tone");
            set_raw(&mut x, "mix", g, "Mix");
            x
        }
        "Rack_StudioChamber" => {
            let mut x = b("room")?;
            set_dial(&mut x, "size", g, "Time");
            set_dial(&mut x, "tone", g, "Tone");
            set_raw(&mut x, "mix", g, "Mix");
            x
        }
        "Pedal_AnalogueDelay"
        | "Pedal_NPNDelay"
        | "Bass_Pedal_BassFilterDelay"
        | "Bass_Pedal_BassFilterEcho"
        | "Pedal_ValveEcho"
        | "Pedal_CosmicEcho"
        | "Pedal_OilCanEcho"
        | "Rack_TapeEcho"
        | "Pedal_NoFiEcho"
        | "Rack_StudioDelay"
        | "Pedal_ModDelay" => {
            let t = millis(g, "Time").or_else(|| millis(g, "TimeL"));
            let mut id = match k {
                "Pedal_AnalogueDelay"
                | "Pedal_NPNDelay"
                | "Bass_Pedal_BassFilterDelay"
                | "Bass_Pedal_BassFilterEcho" => "analog_delay",
                "Pedal_ValveEcho" | "Pedal_CosmicEcho" | "Pedal_OilCanEcho" | "Rack_TapeEcho" => "tape_echo",
                "Pedal_ModDelay" => "mod_delay",
                _ => "digital_delay",
            };
            // a time past the model's reach: the next darkest one that gets there
            let t_ms = t.unwrap_or(0.0);
            if id == "analog_delay" && t_ms > 600.0 {
                id = "tape_echo";
            }
            if id == "tape_echo" && t_ms > 800.0 {
                id = "digital_delay";
            }
            let mut x = b(id)?;
            if let Some(t) = t {
                x.set("time", t);
            }
            set_dial(&mut x, "repeats", g, "Feedback");
            set_raw(&mut x, "mix", g, "Mix");
            match k {
                "Pedal_ModDelay" => {
                    set_hz(&mut x, "rate", g, "Rate", 0.1, 3.0);
                    set_dial(&mut x, "depth", g, "Depth");
                }
                "Pedal_NoFiEcho" => {
                    x.set("tone", 1.5);
                }
                "Rack_StudioDelay" => {
                    if let Some(f) = g.knob("Filter") {
                        x.set("tone", 10.0 - f / 10.0);
                    }
                }
                // a tape or analog echo moved to the digital one keeps its dark repeats
                _ if id == "digital_delay" => {
                    x.set("tone", 3.0);
                }
                _ => {}
            }
            x
        }
        "Pedal_GermaniumDrive"
        | "Pedal_CustomDrive"
        | "Pedal_LineDrive"
        | "Pedal_SuperDrive"
        | "Pedal_Distortion"
        | "Pedal_VintageDistortion"
        | "Pedal_MetalDistortion"
        | "Pedal_MarshallGuvnorPlus" => {
            let id = match k {
                // "classic smooth overdrive"
                "Pedal_GermaniumDrive" => "germanium",
                // "a custom overdrive for smooth riffs", and the '80s Super Drive (a Boss SD-1): Screamer kin
                "Pedal_CustomDrive" | "Pedal_SuperDrive" => "green",
                // "crunchy dense distortion/overdrive with a bright character"
                "Pedal_LineDrive" => "gold",
                // "a relatively low-gain crunchy distortion": an op-amp hard clipper
                "Pedal_VintageDistortion" => "rodent",
                "Pedal_MetalDistortion" => "metal",
                // "Distortion", and Marshall's Guv'nor
                _ => "orange",
            };
            let mut x = b(id)?;
            let (gain_key, tone_key) = match id {
                "green" => ("drive", "tone"),
                "orange" => ("dist", "tone"),
                "gold" => ("gain", "treble"),
                "rodent" => ("distortion", "filter"),
                _ => ("gain", "tone"),
            };
            set_dial(&mut x, gain_key, g, "Gain");
            if id == "rodent" {
                // the RAT's Filter cuts treble as it turns up
                if let Some(t) = dial(g, "Tone") {
                    x.set("filter", 10.0 - t);
                }
            } else if k == "Pedal_MarshallGuvnorPlus" {
                set_dial(&mut x, tone_key, g, "Treble");
            } else {
                set_dial(&mut x, tone_key, g, "Tone");
            }
            x
        }
        "Pedal_ShredZone" => {
            let mut x = b("metal")?;
            set_dial(&mut x, "gain", g, "Gain");
            // its EQ is in dB (-9..9)
            for (key, name) in [("low", "Bass"), ("mid", "Mid"), ("high", "Treble")] {
                set_raw(&mut x, key, g, name);
            }
            x
        }
        // (P41.1: in a slot after the amp, RS's booster is a level boost for a lead, not a treble booster clipping a
        // preamp-level signal: our clean boost, a dB a step of its Boost)
        "Pedal_RangeBooster" if g.slot.starts_with("Post") || g.slot.starts_with("Rack") => {
            let mut x = b("boost")?;
            x.set("gain", dial(g, "Boost").unwrap_or(5.0)).set("bright", 5.0);
            x
        }
        "Pedal_RangeBooster" => {
            let mut x = b("treble_boost")?;
            set_dial(&mut x, "boost", g, "Boost");
            x
        }
        "Bass_Pedal_BassDistortion" if dial(g, "Gain").unwrap_or(5.0) >= 6.0 => {
            // a distortion turned up: the modern grinder (P37.2)
            let mut x = b("bass_grind")?;
            set_dial(&mut x, "drive", g, "Gain");
            if let Some(t) = dial(g, "Tone") {
                x.set("treble", (t - 5.0) * 2.4);
            }
            match g.knob("Blend") {
                Some(v) => x.set("blend", v),
                None => x.set("blend", 100.0),
            };
            x
        }
        "Bass_Pedal_BassOverdrive" | "Bass_Pedal_BassDistortion" => {
            let mut x = b("bass_drive")?;
            set_dial(&mut x, "gain", g, "Gain");
            set_dial(&mut x, "tone", g, "Tone");
            match g.knob("Blend") {
                Some(v) => x.set("blend", v),
                None => x.set("blend", 100.0),
            };
            x
        }
        "Pedal_CaptFuzzle" => {
            // "a highly crunchy nasty fuzz"; its Gain goes to 150
            let mut x = b("round_fuzz")?;
            if let Some(v) = g.knob("Gain") {
                x.set("fuzz", v / 15.0);
            }
            x
        }
        "Pedal_BuzzOne" => {
            // "a basic fuzz"
            let mut x = b("bender")?;
            set_dial(&mut x, "attack", g, "Gain");
            x
        }
        "Bass_Pedal_BassFuzz" => {
            // the bass Muff (P37.2: the guitar one took the bass's lows out)
            let mut x = b("bass_muff")?;
            set_dial(&mut x, "sustain", g, "Gain");
            set_dial(&mut x, "tone", g, "Tone");
            if let Some(v) = g.knob("Blend").or_else(|| g.knob("Mix")) {
                x.set("blend", v);
            }
            x
        }
        "Pedal_BuzzToo" | "Pedal_FuzzWasHe" => {
            // "an ultra-dense wooly fuzz", "tons of fuzz with extended tone control": a Muff
            let mut x = b("big_pi")?;
            set_dial(&mut x, "sustain", g, "Gain");
            set_dial(&mut x, "tone", g, "Tone");
            x
        }
        "Pedal_OctaveUp" | "Pedal_Octavius" | "Bass_Pedal_BassSubOctave" => {
            // octave dividers: one octave up, or one down
            let mut x = b("octaver")?;
            let mix = dial(g, "Mix").unwrap_or(6.6);
            if k == "Pedal_OctaveUp" {
                x.set("up", mix).set("sub", 0.0);
            } else {
                x.set("sub", mix).set("up", 0.0);
            }
            x
        }
        "Pedal_MultiPitch" | "Rack_StudioPitch" => {
            let p = g.knob("Pitch1").or_else(|| g.knob("Pitch")).unwrap_or(-12.0);
            let up = octave_of(p)?;
            let mut x = b("octaver")?;
            let mix = dial(g, "Mix").unwrap_or(5.0);
            if up {
                x.set("up", mix).set("sub", 0.0);
            } else {
                x.set("sub", mix).set("up", 0.0);
            }
            x
        }
        "Pedal_USWah" | "Pedal_ModernWah" | "Pedal_UKWah" | "Bass_Pedal_BassWah" | "Rack_StudioWahFilter" => {
            let mut x = b("wah")?;
            set_dial(&mut x, "position", g, "Pedal");
            if let Some(a) = g.knob("Auto") {
                x.set("mode", if a >= 0.5 { 1.0 } else { 0.0 });
            }
            set_dial(&mut x, "sens", g, "Sens");
            set_dial(&mut x, "speed", g, "Speed");
            x
        }
        "Pedal_BobFilter" | "Pedal_AutoFilter" | "Bass_Pedal_BassAutoFilter" | "Rack_SynthFilter" => {
            let mut x = b("env_filter")?;
            set_dial(&mut x, "sens", g, "Sens");
            set_dial(&mut x, "res", g, "Res");
            // in ms, onto the filter's own curves (1..60 ms, 30..600 ms)
            set_hz(&mut x, "attack", g, "Attack", 1.0, 60.0);
            set_hz(&mut x, "release", g, "Release", 30.0, 600.0);
            // LP, BP, HP: a high-pass sweep is closest to the band-pass
            if let Some(t) = g.knob("FilterType").or_else(|| g.knob("Filter")) {
                x.set("mode", if t >= 0.5 { 1.0 } else { 0.0 });
            }
            x
        }
        // (P41.1: a LoFi turned down to 1% or less does nothing but sit on the board)
        "Pedal_LoFiFilter" | "Pedal_BitCruncher" if g.knob("Mix").is_some_and(|m| m <= 1.0) => {
            return Some(Vec::new());
        }
        "Pedal_LoFiFilter" | "Pedal_BitCruncher" => {
            let mut x = b("lofi")?;
            x.set("type", if k == "Pedal_BitCruncher" { 2.0 } else { 0.0 });
            set_raw(&mut x, "mix", g, "Mix");
            x
        }
        "Pedal_AcousticEmulator" => {
            let mut x = b("acoustic")?;
            set_dial(&mut x, "body", g, "Body");
            set_dial(&mut x, "top", g, "Tone");
            x
        }
        "Pedal_Chorus"
        | "Pedal_Chorus20"
        | "Pedal_DigitalChorus"
        | "Pedal_VintageChorus"
        | "Rack_StudioChorus"
        | "Bass_Pedal_BassChorus"
        | "Bass_Pedal_BassEnbig" => {
            let mut x = b("chorus")?;
            set_hz(&mut x, "rate", g, "Rate", 0.12, 6.0);
            set_dial(&mut x, "depth", g, "Depth");
            set_raw(&mut x, "mix", g, "Mix");
            if k.starts_with("Bass_") {
                // the CEB-3's low filter: the bass's lows stay dry (P37.2)
                x.set("low_cut", 3.0);
            }
            x
        }
        "Pedal_Enbiggenator" | "Pedal_SendInTheClones" => {
            // "makes your tone BIG", "clones your tone": a thick chorus (their Rate is a 0-100 knob)
            let mut x = b("chorus")?;
            set_dial(&mut x, "rate", g, "Rate");
            set_dial(&mut x, "depth", g, "Depth");
            set_raw(&mut x, "mix", g, "Mix");
            x
        }
        k if k.contains("Flanger") => {
            let mut x = b("flanger")?;
            if k == "Rack_StudioFlanger" {
                set_hz(&mut x, "rate", g, "Rate", 0.05, 4.0);
            } else {
                set_dial(&mut x, "rate", g, "Rate");
            }
            set_dial(&mut x, "depth", g, "Depth");
            set_dial(&mut x, "regen", g, "Regen");
            set_raw(&mut x, "mix", g, "Mix");
            x
        }
        "Pedal_Phaser"
        | "Pedal_ShaverPhaser"
        | "Pedal_PlanePhase"
        | "Bass_Pedal_BassPhase"
        | "Rack_StereoPhaser" => {
            let mut x = b("phaser")?;
            if k == "Rack_StereoPhaser" {
                set_hz(&mut x, "speed", g, "Rate", 0.08, 8.0);
            } else {
                set_dial(&mut x, "speed", g, "Rate");
            }
            set_dial(&mut x, "depth", g, "Depth");
            set_raw(&mut x, "mix", g, "Mix");
            x
        }
        "Pedal_AmpTrem" | "Pedal_MultiTrem" | "Pedal_Tremolo" | "Rack_StereoTubeTrem" => {
            let mut x = b("tremolo")?;
            set_dial(&mut x, "speed", g, "Speed");
            // the amp trem has a Depth; the others a Mix that is how deep it goes
            set_dial(&mut x, "depth", g, "Mix");
            set_dial(&mut x, "depth", g, "Depth");
            if let Some(w) = g.knob("Waveform") {
                // sine, triangle, square (and the multi-trem's short and long squares)
                x.set("wave", w.round().clamp(0.0, 2.0));
            }
            x
        }
        "Pedal_BakedRotatoe" | "Rack_RotaVibe" => {
            let mut x = b("rotary")?;
            if let Some(r) = g.knob("Rate") {
                x.set("speed", if r >= 50.0 { 1.0 } else { 0.0 });
            }
            set_dial(&mut x, "balance", g, "Balance");
            set_dial(&mut x, "depth", g, "Depth");
            set_raw(&mut x, "mix", g, "Mix");
            x
        }
        "Pedal_OmniMod" | "Pedal_MarshallSupervibe" => {
            let mut x = b("vibe")?;
            set_dial(&mut x, "speed", g, "Rate");
            set_dial(&mut x, "intensity", g, "Depth");
            x
        }
        "Pedal_AmpVibe" | "Pedal_MultiVibe" | "Rack_StereoAnalogVibe" => {
            // amp-style vibrato: pitch, not a chorus
            let mut x = b("vibe")?;
            set_dial(&mut x, "speed", g, "Speed");
            set_dial(&mut x, "intensity", g, "Mix");
            x.set("mode", 1.0);
            x
        }
        _ => return None,
    };
    blk.on = true;
    Some(vec![blk])
}

/// The models standing in for one piece of a tone's gear, its knobs carried over: usually one; none for gear
/// that is flat on purpose (a PA cab, a guitar's DI preamp); an amp with a built-in EQ or compressor brings
/// those too. None: nothing here stands in for it.
pub fn blocks_for(g: &Gear, tone: &RigTone) -> Option<Vec<Block>> {
    matched(g, tone).0
}

/// How a piece of a tone's gear is matched (P38.2, the library coverage report and the room's song tone view).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Match {
    /// Mapped by what it is (a known amp, cab or pedal).
    Known,
    /// An amp or cab no table knows: its voicing guessed from the tone and its name, its speakers from its digits.
    Guessed,
    /// A DI or PA on a bass or acoustic part: no block, as the song has it.
    Flat,
    /// A DI or PA on an electric part: our cleanest amp or an open 1x12 stands in (D141).
    StandIn,
    /// Nothing here stands in for it (a ring mod, a harmony): left out.
    LeftOut,
}

impl Match {
    pub fn label(self) -> &'static str {
        match self {
            Match::Known => "known",
            Match::Guessed => "guessed",
            Match::Flat => "flat on purpose",
            Match::StandIn => "stand-in",
            Match::LeftOut => "left out",
        }
    }
}

/// How `g` is matched in `tone`.
pub fn match_of(g: &Gear, tone: &RigTone) -> Match {
    matched(g, tone).1
}

fn matched(g: &Gear, tone: &RigTone) -> (Option<Vec<Block>>, Match) {
    let bass = is_bass_tone(tone);
    // (P37.6c: RS's acoustic tones often use the bass multiband compressor; on a guitar it becomes our clean
    // studio compressor, not a bass unit)
    if g.key == "Bass_Pedal_MBComp" && !bass {
        let c = g.knob("Compress").unwrap_or(40.0).clamp(0.0, 100.0);
        let Some(mut x) = Block::of("studio_comp") else {
            return (None, Match::LeftOut);
        };
        x.set("threshold", -6.0 - 0.3 * c)
            .set("ratio", 3.0)
            .set("makeup", 0.1 * c);
        x.on = true;
        return (Some(vec![x]), Match::Known);
    }
    // (P41.1: RS puts its acoustic emulator on some electric parts to thin a guitar out, Seven Nation Army's lead;
    // played through an amp it is only a boomy filter: left out on an electric part)
    if g.key == "Pedal_AcousticEmulator" && !tone.has("ACOUSTIC") && !bass {
        return (Some(Vec::new()), Match::Flat);
    }
    let out = match g.slot.as_str() {
        "Amp" => amp(g, tone),
        "Cabinet" => cab(g),
        _ => effect(g),
    };
    // (P41.1: a bass part on one of RS's guitar cabs, Another One Bites the Dust's 1x12: a bass cab of the same
    // size, half DI, so the bass keeps its low end)
    let out = match out {
        Some(v) if bass && g.slot == "Cabinet" && v.iter().any(|b| GUITAR_CABS.contains(&b.def().id)) => {
            let id = match v[0].def().id {
                "cab_4x12" => "cab_4x10",
                _ => "cab_2x10",
            };
            return (Block::of(id).map(|b| vec![b.with("di", 50.0)]), Match::StandIn);
        }
        o => o,
    };
    let known = match g.slot.as_str() {
        "Amp" => amp_known(&g.key) && (!is_bass_amp(&g.key) || bass_amp_known(&g.key)),
        "Cabinet" => cab_id(&g.key).is_some(),
        _ => true,
    };
    // (P37.6c, D141: an electric guitar tone always gets an amp and a cab: a DI preamp or a PA speaker RS
    // uses on an electric part becomes our cleanest amp or an open 1x12, not "nothing")
    let electric = !bass && !tone.has("ACOUSTIC");
    let stand_in = |id: &str| Block::of(id).map(|b| vec![b]);
    match (out, g.slot.as_str()) {
        (Some(v), "Amp") if v.is_empty() && electric => {
            let gain = g.knob("Gain").map(|v| (v / 10.0).clamp(0.0, 10.0)).unwrap_or(3.0);
            (
                Block::of("amp_clean").map(|b| vec![b.with("gain", gain.min(4.0))]),
                Match::StandIn,
            )
        }
        (Some(v), "Cabinet") if v.is_empty() && electric => (stand_in("cab_1x12"), Match::StandIn),
        // (P38.2: a bass straight into a PA, as RS has it, is still a bass through a cab: one that's mostly DI, so
        // it sounds as recorded and has a cab to swap for an IR)
        (Some(v), "Cabinet") if v.is_empty() && bass => (
            Block::of("cab_4x10").map(|b| vec![b.with("di", BASS_DI_PCT)]),
            Match::StandIn,
        ),
        (None, "Cabinet") if bass => (
            Block::of("cab_4x10").map(|b| vec![b.with("di", BASS_DI_PCT)]),
            Match::StandIn,
        ),
        (None, "Amp") if electric => (stand_in("amp_clean"), Match::StandIn),
        (None, "Cabinet") if electric => (stand_in("cab_1x12"), Match::StandIn),
        (Some(v), _) if v.is_empty() => (Some(v), Match::Flat),
        (Some(v), _) => (Some(v), if known { Match::Known } else { Match::Guessed }),
        (None, _) => (None, Match::LeftOut),
    }
}

/// A clean or low-gain tone (P41.1): no drive or fuzz on before the amp, and the amp itself clean (a clean, tweed,
/// chime or bass voice under 5, any other under `LOW_GAIN`). Such a tone gets no automatic gate.
pub fn clean_tone(b: &Board) -> bool {
    let Some(i) = b
        .blocks
        .iter()
        .position(|x| x.on && x.def().kind == super::Kind::Amp)
    else {
        return !b
            .blocks
            .iter()
            .any(|x| x.on && matches!(x.def().kind, super::Kind::Drive | super::Kind::Fuzz));
    };
    if b.blocks[..i]
        .iter()
        .any(|x| x.on && matches!(x.def().kind, super::Kind::Drive | super::Kind::Fuzz))
    {
        return false;
    }
    let amp = &b.blocks[i];
    let gain = amp.get("gain").or_else(|| amp.get("volume")).unwrap_or(5.0);
    match amp.def().id {
        "amp_clean" | "amp_tweed" | "amp_chime" => gain < 5.0,
        id if id.contains("bass") || id == "amp_b15" => gain < 5.0,
        _ => gain < LOW_GAIN,
    }
}

/// RS's Octavia (its octave-up pedal) next to a fuzz is Hendrix's Octavia: our octave fuzz in the fuzz's place, at its
/// fuzz (P41.1, the gigging guitarist's #5; Purple Haze's solo).
fn octavia_into_fuzz(blocks: &mut Vec<Block>, keys: &[(u32, String)]) {
    let key = |b: &Block| keys.iter().find(|(u, _)| *u == b.uid).map(|(_, k)| k.as_str());
    let amp = blocks
        .iter()
        .position(|b| b.def().kind == super::Kind::Amp)
        .unwrap_or(blocks.len());
    let Some(o) = blocks[..amp]
        .iter()
        .position(|b| key(b) == Some("Pedal_OctaveUp") && b.get("up").unwrap_or(0.0) > 0.0)
    else {
        return;
    };
    let Some(f) = blocks[..amp]
        .iter()
        .position(|b| b.def().kind == super::Kind::Fuzz)
    else {
        return;
    };
    let fuzz = &blocks[f];
    let amount = ["fuzz", "sustain", "attack", "gain"]
        .iter()
        .find_map(|k| fuzz.get(k))
        .unwrap_or(7.0);
    let Some(mut of) = Block::of("octave_fuzz") else {
        return;
    };
    of.set("fuzz", amount);
    of.on = fuzz.on;
    blocks[f] = of;
    blocks.remove(o);
}

/// Our guitar cabs (a bass part on one gets a bass cab instead).
const GUITAR_CABS: [&str; 4] = ["cab_1x10", "cab_1x12", "cab_2x12", "cab_4x12"];

/// The DI share of the bass cab standing in for a PA or a missing cab on a bass part (P38.2): mostly the DI, as such
/// a part is recorded.
pub const BASS_DI_PCT: f32 = 80.0;

/// The board for an RS tone. Unknown gear is skipped.
pub fn board_for(tone: &RigTone) -> Board {
    board_keyed(tone).0
}

/// The board for an RS tone and, for each block, the gear key it stands for (None for what the board adds:
/// a gate, an acoustic sim and its room). For the NAM picker (P33.2), which swaps gear for captures of it.
pub fn board_keyed(tone: &RigTone) -> (Board, Vec<Option<String>>) {
    let mut blocks: Vec<Block> = Vec::new();
    let mut keys: Vec<(u32, String)> = Vec::new();
    for g in &tone.gear {
        match blocks_for(g, tone) {
            Some(bs) => {
                for b in bs {
                    if blocks.len() < MAX_BLOCKS {
                        keys.push((b.uid, g.key.clone()));
                        blocks.push(b);
                    }
                }
            }
            None => log::debug!("pedalboard: no model for {} ({})", g.key, tone.key),
        }
    }
    // an acoustic tone with no amp: the simulator and a little room
    if tone.has("ACOUSTIC") && !blocks.iter().any(|b| b.def().kind == super::Kind::Amp) {
        if !blocks.iter().any(|b| b.def().id == "acoustic") {
            blocks.insert(
                0,
                Block::new(super::catalog::find("acoustic").expect("acoustic sim")),
            );
        }
        if !blocks.iter().any(|b| b.def().kind == super::Kind::Reverb) && blocks.len() < MAX_BLOCKS {
            blocks.push(Block::new(super::catalog::find("room").expect("room")).with("mix", 15.0));
        }
    }
    // (P38.2: an electric or bass tone always has an amp and a cab, even when its rig has none: our cleanest amp
    // (a bass's SVT at low gain) and an open 1x12 (a bass cab that's mostly DI))
    let bass = is_bass_tone(tone);
    if !tone.has("ACOUSTIC") || bass {
        let has = |k: super::Kind, bs: &[Block]| bs.iter().any(|b| b.def().kind == k);
        if !has(super::Kind::Amp, &blocks)
            && !tone.has("ACOUSTIC")
            && let Some(a) = Block::of(if bass { "amp_bass" } else { "amp_clean" })
        {
            let at = blocks
                .iter()
                .position(|b| b.def().kind == super::Kind::Cab)
                .unwrap_or(blocks.len());
            blocks.insert(at, a.with("gain", if bass { 2.0 } else { 3.0 }));
        }
        if !has(super::Kind::Cab, &blocks) && has(super::Kind::Amp, &blocks) {
            let cab = if bass {
                Block::of("cab_4x10").map(|b| b.with("di", BASS_DI_PCT))
            } else {
                Block::of("cab_1x12")
            };
            if let Some(c) = cab {
                let at = blocks
                    .iter()
                    .position(|b| b.def().kind == super::Kind::Amp)
                    .map(|i| i + 1)
                    .unwrap_or(blocks.len());
                blocks.insert(at, c);
            }
        }
    }
    blocks.truncate(MAX_BLOCKS);
    octavia_into_fuzz(&mut blocks, &keys);
    let mut board = Board {
        blocks,
        level_db: 0.0,
    };
    // high gain hisses between notes: a gate, as a player would put one (RS gates its own amps). (P41.1, D162: none on
    // a clean or low-gain tone, whose decays it would choke; the gate after the amp, keyed on the DI)
    let hiss = if board.blocks.len() < MAX_BLOCKS
        && !board.blocks.iter().any(|b| b.def().id == "gate")
        && !clean_tone(&board)
    {
        super::hiss_gain_db(&board)
    } else {
        0.0
    };
    // (RS's own gate pedal, where RS put it before the amp: after the amp, keyed on the input, like ours)
    if let Some(i) = board.blocks.iter().position(|b| b.def().id == "gate") {
        let g = board.blocks.remove(i);
        let at = super::gate_slot(&board);
        board.blocks.insert(at, g);
    }
    if hiss > super::GATE_HISS_DB {
        // (P38.2: a crunch from 20 dB gets a gentle gate, only closing on the guitar's idle noise; high gain a firm one)
        let gate = super::new_gate(hiss > 28.0);
        let at = super::gate_slot(&board);
        board.blocks.insert(at, gate);
    }
    // (P41.1, D161) every tone at one loudness, then RS's own volume within 3 dB
    board.level_db = super::level::song_level(&board, bass, tone.volume_db);
    let keyed = board
        .blocks
        .iter()
        .map(|b| keys.iter().find(|(u, _)| *u == b.uid).map(|(_, k)| k.clone()))
        .collect();
    (board, keyed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gear(slot: &str, key: &str, knobs: &[(&str, f32)]) -> Gear {
        Gear {
            slot: slot.into(),
            key: key.into(),
            knobs: knobs.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        }
    }

    fn tone(desc: &[&str], gear: Vec<Gear>) -> RigTone {
        RigTone {
            key: "x_tone".into(),
            name: "x_tone".into(),
            descriptors: desc.iter().map(|d| d.to_string()).collect(),
            volume_db: 0.0,
            gear,
        }
    }

    /// The one block a piece of gear becomes, in a tone of its own.
    fn one(desc: &[&str], g: Gear) -> Block {
        let t = tone(desc, vec![g.clone()]);
        let mut bs = blocks_for(&g, &t).expect("mapped");
        assert_eq!(bs.len(), 1, "{:?}", ids(&bs));
        bs.remove(0)
    }

    fn ids(bs: &[Block]) -> Vec<&'static str> {
        bs.iter().map(|b| b.def().id).collect()
    }

    fn near(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-3)
    }

    #[test]
    fn code_named_amps_are_what_they_are() {
        // EN-30 is an AC30 whatever the tone calls itself; its presence becomes the Cut, turned the other way
        for d in [&["CLEAN"][..], &["DISTORTION"], &["HIGH GAIN"]] {
            let b = one(
                d,
                gear(
                    "Amp",
                    "Amp_EN30",
                    &[("Gain", 70.0), ("Treble", 80.0), ("Pres", 30.0)],
                ),
            );
            assert_eq!(b.def().id, "amp_chime");
            assert!(near(b.get("gain"), 7.0) && near(b.get("treble"), 8.0) && near(b.get("cut"), 7.0));
        }
        // the tweeds: Gain is the volume, the treble the one tone knob
        let b = one(
            &["CLEAN"],
            gear(
                "Amp",
                "Amp_TW22",
                &[("Gain", 40.0), ("Treble", 60.0), ("Bass", 50.0)],
            ),
        );
        assert_eq!(b.def().id, "amp_tweed");
        assert!(near(b.get("volume"), 4.0) && near(b.get("tone"), 6.0));
        assert_eq!(
            one(&[], gear("Amp", "Amp_TW40", &[("Gain", 90.0)])).def().id,
            "amp_tweed"
        );
        assert_eq!(
            one(&["DISTORTION"], gear("Amp", "Amp_CS90", &[("Gain", 90.0)]))
                .def()
                .id,
            "amp_clean"
        );
        assert_eq!(
            one(&["CLEAN"], gear("Amp", "Amp_HG500", &[("Gain", 60.0)]))
                .def()
                .id,
            "amp_snake"
        );
        // (P41.2, D167: the Mark, the SLO, the Orange and the 4x10 have their own voices now)
        for (key, id) in [
            ("Amp_CA85", "amp_cali"),
            ("Amp_HG100", "amp_snake"),
            ("Amp_BT45", "amp_snake"),
            ("Amp_OrangeOR100", "amp_tangerine"),
            ("Amp_OrangeRockerverb", "amp_tangerine"),
            ("Amp_MarshallJCM800", "amp_lead"),
            ("Amp_HiwattCustom100", "amp_surrey"),
        ] {
            assert_eq!(
                one(&["DISTORTION"], gear("Amp", key, &[("Gain", 70.0)])).def().id,
                id,
                "{key}"
            );
        }
        assert_eq!(
            one(&[], gear("Amp", "Amp_GibsonGA8", &[("Gain", 88.0)])).def().id,
            "amp_tweed"
        );
        assert!(amp_known("Amp_CA38") && amp_known("Bass_Amp_HT300B") && !amp_known("Amp_Future99"));
        assert!(real_gear("Amp_EN30").is_some_and(|r| r.contains("AC30")));
    }

    #[test]
    fn channel_amps_turned_down_are_clean_unless_the_tone_is_driven() {
        let rect = |gain: f32| gear("Amp", "Amp_CA100", &[("Gain", gain)]);
        assert_eq!(one(&["CLEAN"], rect(15.0)).def().id, "amp_clean");
        assert_eq!(one(&["CLEAN"], rect(40.0)).def().id, "amp_clean");
        assert_eq!(one(&["CHORUS"], rect(15.0)).def().id, "amp_clean");
        // a Rectifier at 20 is still a crunch when the tone says it's driven
        assert_eq!(one(&["DISTORTION"], rect(20.0)).def().id, "amp_modern");
        assert_eq!(one(&["LEAD"], rect(80.0)).def().id, "amp_modern");
        // a Plexi has no clean channel
        let plexi = gear(
            "Amp",
            "Amp_MarshallPlexi",
            &[("Loudness1", 10.0), ("Loudness2", 20.0)],
        );
        let b = one(&["CLEAN"], plexi);
        assert_eq!(b.def().id, "amp_crunch");
        assert!(near(b.get("gain"), 2.0));
    }

    #[test]
    fn unknown_amps_fall_back_to_the_tone() {
        assert_eq!(
            one(&["HIGH GAIN"], gear("Amp", "Amp_Future99", &[("Gain", 80.0)]))
                .def()
                .id,
            "amp_modern"
        );
        assert_eq!(
            one(&["CLEAN"], gear("Amp", "Amp_Future99", &[("Gain", 20.0)]))
                .def()
                .id,
            "amp_clean"
        );
    }

    #[test]
    fn bass_amps_keep_their_units_and_built_in_extras() {
        // a dB tone stack: flat (0 dB) is the dial's middle
        let b = one(
            &["BASS"],
            gear(
                "Amp",
                "Bass_Amp_HT400B",
                &[("Gain", 50.0), ("Bass", 6.0), ("Mid", -12.0), ("Treble", 0.0)],
            ),
        );
        assert!(near(b.get("bass"), 7.5) && near(b.get("mid"), 0.0) && near(b.get("treble"), 5.0));
        // a 0-100 stack, the five-way mid switch, Ultra Hi
        let b = one(
            &["BASS"],
            gear(
                "Amp",
                "Bass_Amp_HT300B",
                &[
                    ("Bass", 60.0),
                    ("Treble", 50.0),
                    ("MidFreq", 3.0),
                    ("HiShift", 1.0),
                ],
            ),
        );
        assert!(near(b.get("bass"), 6.0) && near(b.get("treble"), 5.0) && near(b.get("freq"), 1600.0));
        assert!(near(b.get("ultra_hi"), 1.0) && near(b.get("ultra_lo"), 0.0));
        // the three-way switch; a mid frequency in Hz
        let b = one(&["BASS"], gear("Amp", "Bass_Amp_CH300B", &[("MidFreq", 2.0)]));
        assert!(near(b.get("freq"), 3000.0));
        let b = one(&["BASS"], gear("Amp", "Bass_Amp_CH300B", &[("MidFreq", 480.0)]));
        assert!(near(b.get("freq"), 480.0));
        // (P37.2) each bass amp on its own voice; the SVT's Ultra switches are its own
        let b = one(&["BASS"], gear("Amp", "Bass_Amp_LT25B", &[("Gain", 30.0)]));
        assert_eq!(b.def().id, "amp_b15");
        assert!(near(b.get("volume"), 3.0));
        assert!(near(
            one(&["BASS"], gear("Amp", "Bass_Amp_HT300B", &[("HiShift", 1.0)])).get("ultra_hi"),
            1.0
        ));
        assert_eq!(
            one(&["BASS"], gear("Amp", "Bass_Amp_BT975B", &[])).def().id,
            "amp_bass_tube"
        );
        assert_eq!(
            one(&["BASS"], gear("Amp", "Bass_Amp_CS75B", &[])).def().id,
            "amp_bass_ss"
        );
        assert_eq!(bass_voicing("Bass_Amp_Future99"), "amp_bass");
        // a graphic EQ and a compressor built in: the compressor first, the EQ after, on the nearest bands
        let g = gear(
            "Amp",
            "Bass_Amp_CH600B",
            &[
                ("Compression", 55.0),
                ("30", -4.0),
                ("90", 2.0),
                ("800", -6.0),
                ("7500", 0.0),
            ],
        );
        let t = tone(&["BASS"], vec![g.clone()]);
        let bs = blocks_for(&g, &t).unwrap();
        assert_eq!(ids(&bs), vec!["bass_comp", "amp_bass_ss", "bass_eq8"]);
        assert!(near(bs[0].get("compress"), 5.5));
        assert!(
            near(bs[2].get("b30"), -4.0) && near(bs[2].get("b75"), 2.0) && near(bs[2].get("b1100"), -6.0)
        );
        // the Eden's parametric low and high bands
        let g = gear(
            "Amp",
            "Bass_Amp_EdenWT550",
            &[("Lo", -9.0), ("LoFreq", 220.0), ("Hi", 3.0), ("HiFreq", 1.6)],
        );
        let bs = blocks_for(&g, &tone(&["BASS"], vec![g.clone()])).unwrap();
        assert_eq!(ids(&bs), vec!["amp_bass_ss", "studio_eq"]);
        assert!(near(bs[1].get("bass"), -9.0) && near(bs[1].get("bass_freq"), 220.0));
        assert!(near(bs[1].get("hi_freq"), 1600.0));
        // the SansAmp DI
        let b = one(
            &["BASS"],
            gear("Amp", "DI_Amp_BassDriver", &[("Gain", 40.0), ("Blend", 30.0)]),
        );
        assert_eq!(b.def().id, "bass_drive");
        assert!(near(b.get("blend"), 30.0));
    }

    #[test]
    fn a_di_preamp_is_flat_for_a_guitar_and_a_bass_amp_for_a_bass() {
        let acoustic = tone(
            &["ACOUSTIC"],
            vec![
                gear("PrePedal1", "Pedal_AcousticEmulator", &[("Body", 60.0)]),
                gear("Amp", "DI_Amp_TubePre", &[("Gain", 10.0)]),
                // RS's acoustic tones often use the bass multiband compressor: still a guitar
                gear("PostPedal1", "Bass_Pedal_MBComp", &[("Compress", 40.0)]),
                gear("Cabinet", "Cab_PA600C", &[]),
            ],
        );
        let b = board_for(&acoustic);
        assert_eq!(ids(&b.blocks), vec!["acoustic", "studio_comp", "room"]);
        let bass = tone(
            &["BASS"],
            vec![
                gear("Amp", "DI_Amp_TubePre", &[("Gain", 10.0)]),
                gear("Cabinet", "Cab_PA999C", &[]),
            ],
        );
        // (P38.2: a bass into a PA gets a bass cab, mostly DI)
        assert_eq!(ids(&board_for(&bass).blocks), vec!["amp_bass", "cab_4x10"]);
    }

    #[test]
    fn cabs_by_their_speakers_and_mic() {
        let cab = |key: &str| one(&[], gear("Cabinet", key, &[]));
        // a Bassman 4x10 is open-backed: the open 2x12, not the closed 4x12
        assert_eq!(cab("Cab_TW410C_57_Edge").def().id, "cab_4x10_open");
        assert_eq!(cab("Cab_AT0112C_57_Cone").def().id, "cab_1x12");
        assert_eq!(cab("Cab_Marshall1936_Ribbon_Cone").def().id, "cab_4x12");
        assert_eq!(cab("Cab_CS1515C_57_Edge").def().id, "cab_2x12");
        assert_eq!(cab("Bass_Cab_GB415BC_Tube_Edge").def().id, "cab_4x15");
        assert_eq!(cab("Bass_Cab_CH410BC_57_Edge").def().id, "cab_4x10");
        assert_eq!(cab("Bass_Cab_CH210BC_57_Edge").def().id, "cab_2x10");
        assert_eq!(cab("Bass_Cab_XY410BC_57_Edge").def().id, "cab_4x10");
        assert_eq!(cab("Bass_Cab_EdenD610XST_57_Edge").def().id, "cab_8x10");
        // mics: a tube mic is a condenser; on the cone is bright, off axis darker and further back
        let b = cab("Cab_TW112C_Tube_OffAxis");
        assert!(near(b.get("mic_type"), 2.0) && near(b.get("mic"), 1.0) && near(b.get("distance"), 3.5));
        let b = cab("Cab_EN212C_Ribbon_Cone");
        assert!(near(b.get("mic_type"), 1.0) && near(b.get("mic"), 7.0));
        // an unknown cab by its digits; a PA is flat; a boom box is the lo-fi radio
        assert_eq!(cab("Cab_ZZ412C_57_Edge").def().id, "cab_4x12");
        let pa = gear("Cabinet", "Cab_PA999C", &[]);
        assert!(blocks_for(&pa, &tone(&["ACOUSTIC"], vec![])).unwrap().is_empty());
        // (on an electric part a PA becomes an open 1x12, never "nothing", D141)
        assert_eq!(
            blocks_for(&pa, &tone(&[], vec![])).unwrap()[0].def().id,
            "cab_1x12"
        );
        assert_eq!(cab("Cab_BoomBox").def().id, "lofi");
    }

    #[test]
    fn effect_knobs_convert_to_our_units() {
        use crate::board::dsp::sweep;
        // a ratio switch, not a ratio
        let b = one(
            &[],
            gear(
                "Rack1",
                "Rack_StudioCompressor",
                &[("Ratio", 2.0), ("Threshold", -22.5)],
            ),
        );
        assert!(near(b.get("ratio"), 4.0) && near(b.get("threshold"), -22.5));
        // a chorus rate in Hz lands on the chorus's own rate curve
        let b = one(
            &[],
            gear(
                "PostPedal1",
                "Pedal_Chorus",
                &[("Rate", 0.5), ("Depth", 30.0), ("Mix", 85.0)],
            ),
        );
        let u = b.get("rate").unwrap() / 10.0;
        assert!((sweep(0.12, 6.0, u) - 0.5).abs() < 1e-3);
        assert!(near(b.get("depth"), 3.0) && near(b.get("mix"), 85.0));
        // a 0-100 rate is a dial
        let b = one(
            &[],
            gear(
                "PostPedal1",
                "Pedal_ModernFlanger",
                &[("Rate", 25.0), ("Regen", 90.0)],
            ),
        );
        assert!(near(b.get("rate"), 2.5) && near(b.get("regen"), 9.0));
        // the Shred Zone's EQ is in dB already
        let b = one(
            &[],
            gear("PrePedal1", "Pedal_ShredZone", &[("Bass", -7.0), ("Treble", 3.0)]),
        );
        assert!(near(b.get("low"), -7.0) && near(b.get("high"), 3.0));
        // the gate's threshold in dB
        let b = one(
            &[],
            gear(
                "PrePedal1",
                "Pedal_NoiseGate",
                &[("Thresh", -60.0), ("Rate", 100.0)],
            ),
        );
        assert!(near(b.get("threshold"), -60.0) && near(b.get("release"), 10.0));
        // the envelope filter's ms onto its curves; HP is the band-pass
        let b = one(
            &[],
            gear(
                "PrePedal1",
                "Pedal_BobFilter",
                &[("Attack", 60.0), ("Release", 30.0), ("Filter", 2.0)],
            ),
        );
        assert!(near(b.get("attack"), 10.0) && near(b.get("release"), 0.0) && near(b.get("mode"), 1.0));
        // the multi-trem's five waves onto three
        let b = one(
            &[],
            gear(
                "PostPedal1",
                "Pedal_MultiTrem",
                &[("Waveform", 4.0), ("Mix", 80.0)],
            ),
        );
        assert!(near(b.get("wave"), 2.0) && near(b.get("depth"), 8.0));
        // octaves up and down; a fifth has no counterpart
        let b = one(&[], gear("PrePedal1", "Pedal_Octavius", &[("Mix", 50.0)]));
        assert!(near(b.get("sub"), 5.0) && near(b.get("up"), 0.0));
        let b = one(&[], gear("PrePedal1", "Pedal_OctaveUp", &[("Mix", 66.0)]));
        assert!(near(b.get("up"), 6.6) && near(b.get("sub"), 0.0));
        let fifth = gear("PrePedal1", "Pedal_MultiPitch", &[("Pitch1", -7.0)]);
        assert!(blocks_for(&fifth, &tone(&[], vec![])).is_none());
        let b = one(
            &[],
            gear(
                "PrePedal1",
                "Pedal_MultiPitch",
                &[("Pitch1", 12.0), ("Mix", 40.0)],
            ),
        );
        assert!(near(b.get("up"), 4.0));
        assert!(blocks_for(&gear("PostPedal1", "Pedal_RingMod", &[]), &tone(&[], vec![])).is_none());
        // the fuzzes by what they are; the Captain's Gain goes to 150
        assert_eq!(
            one(&[], gear("PrePedal1", "Pedal_BuzzToo", &[])).def().id,
            "big_pi"
        );
        assert_eq!(
            one(&[], gear("PrePedal1", "Pedal_FuzzWasHe", &[])).def().id,
            "big_pi"
        );
        assert_eq!(
            one(&[], gear("PrePedal1", "Pedal_BuzzOne", &[])).def().id,
            "bender"
        );
        let b = one(&[], gear("PrePedal1", "Pedal_CaptFuzzle", &[("Gain", 150.0)]));
        assert!(near(b.get("fuzz"), 10.0));
        assert_eq!(
            one(&[], gear("PrePedal1", "Pedal_SuperDrive", &[])).def().id,
            "green"
        );
        // Swole is a squashing compressor, not a tremolo
        assert_eq!(
            one(&[], gear("PrePedal1", "Pedal_Swole", &[("Smash", 66.0)]))
                .def()
                .id,
            "squeeze"
        );
        // a delay too long for its model moves to one that reaches
        let d = |key: &str, t: f32| one(&[], gear("PostPedal1", key, &[("Time", t)]));
        assert_eq!(d("Pedal_AnalogueDelay", 400.0).def().id, "analog_delay");
        assert_eq!(d("Pedal_AnalogueDelay", 700.0).def().id, "tape_echo");
        let b = d("Rack_TapeEcho", 940.0);
        assert_eq!(b.def().id, "digital_delay");
        assert!(near(b.get("time"), 940.0));
        // a mod delay's rate in Hz
        let b = one(&[], gear("PostPedal1", "Pedal_ModDelay", &[("Rate", 3.0)]));
        assert!(near(b.get("rate"), 10.0));
    }

    #[test]
    fn sweep_dial_inverts_the_sweep() {
        use crate::board::dsp::sweep;
        for (lo, hi) in [(0.12, 6.0), (30.0, 600.0), (500.0, 10.0)] {
            for d in [0.0, 2.5, 5.0, 10.0] {
                let v = sweep(lo, hi, d / 10.0);
                assert!((sweep_dial(v, lo, hi) - d).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn every_bass_head_in_the_library_is_known() {
        for (k, v) in [
            ("Bass_Amp_HT100B", "amp_bass"),
            ("Bass_Amp_CH350B", "amp_bass_ss"),
            ("Bass_Amp_CS240B", "amp_bass_ss"),
            ("Bass_Amp_CS300B", "amp_bass_ss"),
            ("Bass_Amp_BT600B", "amp_bass_ss"),
            ("Bass_Amp_LT85B", "amp_b15"),
            ("Bass_Amp_OrangeAD200B", "amp_bass_tube"),
        ] {
            let g = gear("Amp", k, &[("Gain", 30.0)]);
            let t = tone(&["BASS"], vec![g.clone()]);
            assert_eq!(match_of(&g, &t), Match::Known, "{k}");
            assert_eq!(bass_voicing(k), v, "{k}");
        }
    }

    #[test]
    fn matches_are_told_apart() {
        let t = tone(&["CLEAN"], vec![]);
        assert_eq!(match_of(&gear("Amp", "Amp_AT20", &[]), &t), Match::Known);
        assert_eq!(match_of(&gear("Amp", "Amp_Nobody99", &[]), &t), Match::Guessed);
        assert_eq!(
            match_of(&gear("Cabinet", "Cab_XY999C_57_Cone", &[]), &t),
            Match::Guessed
        );
        assert_eq!(match_of(&gear("Cabinet", "Cab_PA600C", &[]), &t), Match::StandIn);
        assert_eq!(match_of(&gear("Pedal", "Pedal_RingMod", &[]), &t), Match::LeftOut);
        let ac = tone(&["ACOUSTIC"], vec![]);
        assert_eq!(match_of(&gear("Cabinet", "Cab_PA600C", &[]), &ac), Match::Flat);
    }

    #[test]
    fn a_bass_or_electric_tone_always_has_an_amp_and_a_cab() {
        let kinds = |b: &Board| b.blocks.iter().map(|x| x.def().kind).collect::<Vec<_>>();
        // a bass into a PA: a bass cab that's mostly DI
        let t = tone(
            &["BASS"],
            vec![
                gear("Amp", "Bass_Amp_HT300B", &[("Gain", 30.0)]),
                gear("Cabinet", "Cab_PA600C", &[]),
            ],
        );
        let b = board_for(&t);
        let cab = b
            .blocks
            .iter()
            .find(|x| x.def().kind == super::super::Kind::Cab)
            .expect("a cab");
        assert_eq!(cab.def().id, "cab_4x10");
        assert_eq!(cab.get("di"), Some(BASS_DI_PCT));
        // a bass rig of pedals only: an amp and a cab added
        let t = tone(&["BASS"], vec![gear("Pedal", "Bass_Pedal_BassFuzz", &[])]);
        let k = kinds(&board_for(&t));
        assert!(
            k.contains(&super::super::Kind::Amp) && k.contains(&super::super::Kind::Cab),
            "{k:?}"
        );
        // an electric one with no cab
        let t = tone(&["DISTORTION"], vec![gear("Amp", "Amp_AT20", &[("Gain", 60.0)])]);
        let k = kinds(&board_for(&t));
        assert!(k.contains(&super::super::Kind::Cab), "{k:?}");
        // an acoustic one keeps its simulator and no amp
        let t = tone(&["ACOUSTIC"], vec![gear("Cabinet", "Cab_PA600C", &[])]);
        assert!(!kinds(&board_for(&t)).contains(&super::super::Kind::Amp));
    }
}
