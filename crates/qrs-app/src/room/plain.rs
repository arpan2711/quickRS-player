//! The amp room in plain words (P41.3, D171; the newcomer's review): what each unit is ("Overdrive pedal", "British
//! crunch amp", "4x12 speaker box"), a rig as one line ("Overdrive pedal → Marshall-style amp → 4x12 speaker box"),
//! the one-line "what is it" at the top of every info card, and the small glossary behind the room's hoverable
//! words (cab, gate, IR, capture, scene, compare, DI, mic...). New code (no Python source).
//!
//! Brands (D42/D58): "Marshall-style" only in the `brands` build (`catalog::SHOW_BRANDS`); the shared build says
//! "British crunch amp".

use eframe::egui;
use qrs_audio::board::{Block, Board, Kind, MODELS, catalog};

use super::stage::Entry;

/// An amp model's style: (the maker, for the `brands` build; the style in words).
fn amp_style(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "amp_clean" => ("Fender", "American clean"),
        "amp_tweed" => ("Fender tweed", "small tweed"),
        "amp_chime" => ("Vox", "British chime"),
        "amp_crunch" => ("Marshall", "British crunch"),
        "amp_lead" => ("Marshall", "British high-gain"),
        "amp_modern" => ("Mesa", "modern high-gain"),
        "amp_bass" => ("Ampeg", "big tube bass"),
        "amp_b15" => ("Ampeg", "vintage bass"),
        "amp_bass_ss" => ("Gallien-Krueger", "punchy bass"),
        "amp_bass_tube" => ("Orange", "British bass"),
        _ => return None,
    })
}

/// An amp model in words: "Marshall-style amp" (`brands`), "British crunch amp" (shared), else "amp".
fn amp_words(id: &str) -> String {
    match amp_style(id) {
        Some((maker, _)) if catalog::SHOW_BRANDS => format!("{maker}-style amp"),
        Some((_, style)) => format!("{style} amp"),
        None => "amp".into(),
    }
}

/// A cab model's size from its id ("cab_4x12" → "4x12").
fn cab_size(id: &str) -> Option<&str> {
    let s = id.strip_prefix("cab_")?;
    s.contains('x').then_some(s)
}

/// What a model of ours is, in a newcomer's words.
pub fn model_words(id: &str) -> String {
    let Some(d) = MODELS.iter().find(|m| m.id == id) else {
        return "unit".into();
    };
    let w: &str = match (d.family(), id) {
        (_, "amp_nam") => "real amp, captured",
        (_, "pedal_nam") => "real pedal, captured",
        (_, "cab_ir") => "speaker box (IR)",
        (Kind::Amp, _) => return amp_words(id),
        (Kind::Cab, _) => {
            return match cab_size(id) {
                Some(s) if super::rig::is_bass_model(id) => format!("bass {s} speaker box"),
                Some(s) => format!("{s} speaker box"),
                None => "speaker box".into(),
            };
        }
        (_, "gate") => "Noise gate",
        (Kind::Dynamics, _) => "Compressor",
        (_, "boost" | "treble_boost") => "Boost pedal",
        (_, "orange" | "rodent" | "metal") => "Distortion pedal",
        (_, "bass_drive" | "bass_grind") => "Bass drive pedal",
        (Kind::Drive, _) => "Overdrive pedal",
        (_, "octave_fuzz") => "Octave fuzz pedal",
        (Kind::Fuzz, _) => "Fuzz pedal",
        (_, "acoustic") => "Acoustic simulator",
        (Kind::Eq, _) => "EQ",
        (_, "wah") => "Wah pedal",
        (_, "env_filter") => "Auto-wah",
        (_, "lofi") => "Lo-fi filter",
        (Kind::Filter, _) => "Filter",
        (_, "chorus") => "Chorus",
        (_, "flanger") => "Flanger",
        (_, "phaser") => "Phaser",
        (_, "tremolo") => "Tremolo",
        (_, "vibe") => "Vibe",
        (_, "rotary") => "Rotary speaker",
        (_, "octaver") => "Octave pedal",
        (Kind::Modulation, _) => "Modulation",
        (_, "tape_echo") => "Tape echo",
        (Kind::Delay, _) => "Delay",
        (_, "spring") => "Spring reverb",
        (_, "plate") => "Plate reverb",
        (_, "room") => "Room reverb",
        (_, "hall") => "Hall reverb",
        (Kind::Reverb, _) => "Reverb",
    };
    w.to_string()
}

/// What a unit on a board is: a model's words, a capture by the gear it stands for ("British crunch amp, captured").
pub fn unit_words(b: &Block) -> String {
    let def = b.def();
    if def.file && b.asset != 0 {
        let skin = crate::player::board::captures::skin_of(b.asset);
        let like = skin.as_deref().filter(|s| catalog::find(s).is_some());
        return match (def.id, like) {
            ("amp_nam", Some(s)) => format!("{}, captured", model_words(s)),
            ("pedal_nam", Some(s)) => format!("{}, captured", model_words(s)),
            _ => model_words(def.id),
        };
    }
    model_words(def.id)
}

/// Capitalised for the start of a line ("british crunch amp" never: the words start as written, a lower-case first
/// word gets its capital).
fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A rig as one line, the units that are on: "Overdrive pedal → British crunch amp → 4x12 speaker box".
pub fn chain_words(b: &Board) -> String {
    let parts: Vec<String> = b
        .blocks
        .iter()
        .filter(|x| x.on)
        .map(|x| cap(&unit_words(x)))
        .collect();
    if parts.is_empty() {
        return "Nothing on: you hear yourself dry".into();
    }
    parts.join(" \u{2192} ")
}

// -- the glossary -----------------------------------------------------------------------------------------------------

/// The room's words a newcomer meets, each in a line (the hover on a word, the top of an info card).
pub const GLOSSARY: &[(&str, &str)] = &[
    (
        "amp",
        "The amplifier: where most of a sound's character, and its crunch, comes from.",
    ),
    (
        "cab",
        "The speaker box. An amp needs one to sound like an amp, not a fizzy buzz.",
    ),
    (
        "ir",
        "An impulse response: a recording of a real speaker box and its mic, played as your cab.",
    ),
    (
        "capture",
        "A recording of a real amp or pedal at one setting (made with Neural Amp Modeler, NAM): it plays like that \
         gear.",
    ),
    (
        "gate",
        "A noise gate silences hiss and hum while you're not playing, and opens as soon as you play.",
    ),
    (
        "threshold",
        "How loud your playing must be before the gate opens: lower lets softer notes through.",
    ),
    ("drive", "A pedal that pushes the amp into a warm crunch."),
    ("fuzz", "A pedal that turns notes into a thick, buzzing wall."),
    (
        "dynamics",
        "Compressors and gates: they even out loud and soft notes, or quiet the hiss.",
    ),
    (
        "eq",
        "Turns bands of the sound up or down: the lows, the mids, the highs.",
    ),
    (
        "filter",
        "Sweeps a band of the sound, like a wah pedal's \"wow\".",
    ),
    (
        "modulation",
        "Effects that move: chorus, phaser, flanger, tremolo.",
    ),
    ("delay", "Echoes of what you play."),
    ("reverb", "The sound of a room, a hall or a spring around you."),
    (
        "scene",
        "Up to 4 versions of this sound (verse, solo...), picked with keys 1-4.",
    ),
    (
        "compare",
        "Flips between the sound as you're changing it and as you last saved it (the key left of 1).",
    ),
    (
        "di",
        "The clean signal straight from the instrument, blended in as studios record bass.",
    ),
    (
        "mic",
        "The microphone in front of the speaker box: where it points, how far away and which kind all change the \
         tone.",
    ),
    (
        "modeller",
        "A box (an amp modeller) that makes its own amp sound. Set it to a clean preset when quickRS's amps play.",
    ),
    ("dry", "Your instrument as it comes in: no amp, no effects."),
    (
        "input boost",
        "Raises your input's level before the rig, so quiet pickups drive the amps like loud ones.",
    ),
    (
        "calibrate",
        "A few seconds of playing that set your input's level and say what kind of input it is.",
    ),
    (
        "presence",
        "The top of the treble on the amp's power stage: more bite, or less.",
    ),
    ("master", "The amp's overall volume after its gain."),
    ("blend", "How much of the effect against your clean signal."),
    (
        "gain ladder",
        "A capture recorded at several gain settings: its Gain knob steps between them.",
    ),
    (
        "nam",
        "Neural Amp Modeler: a free, open way to record real amps and pedals as captures.",
    ),
];

/// A word's line from the glossary (any case; "cabinet", "impulse response", "noise gate" are their words).
pub fn term(word: &str) -> Option<&'static str> {
    let w = word.trim().to_lowercase();
    let key = match w.as_str() {
        "cabinet" | "cabs" | "speaker box" => "cab",
        "impulse response" | "irs" => "ir",
        "captures" | "nam capture" => "capture",
        "noise gate" => "gate",
        "a/b" | "ab" => "compare",
        "modeler" | "amp modeller" => "modeller",
        "comp" | "compressor" => "dynamics",
        "overdrive" | "distortion" => "drive",
        other => other,
    };
    GLOSSARY.iter().find(|(k, _)| *k == key).map(|(_, l)| *l)
}

/// A hoverable word (P41.3): while the pointer is on `r`, its glossary line is the room's tip (over it).
pub(crate) fn hover(
    tip: &mut Option<(String, String, egui::Pos2, egui::Align2)>,
    ui: &egui::Ui,
    r: egui::Rect,
    word: &str,
) -> bool {
    if !ui.rect_contains_pointer(r) {
        return false;
    }
    let Some(line) = term(word) else { return false };
    *tip = Some((
        line.to_string(),
        String::new(),
        egui::pos2(r.center().x, r.min.y - 4.0),
        egui::Align2::CENTER_BOTTOM,
    ));
    true
}

/// The one line at the top of an info card: what this kind of unit is.
pub fn what_is(e: Entry) -> &'static str {
    let key = match e {
        Entry::Capture(id) => match crate::captures::with(|l| l.get(id).map(|i| i.kind)) {
            Some(crate::captures::Kind::Ir) => "ir",
            _ => "capture",
        },
        Entry::Model(m) => {
            let d = &MODELS[m];
            match d.id {
                "amp_nam" | "pedal_nam" => "capture",
                "cab_ir" => "ir",
                "gate" => "gate",
                _ => match d.family() {
                    Kind::Amp => "amp",
                    Kind::Cab => "cab",
                    Kind::Drive => "drive",
                    Kind::Fuzz => "fuzz",
                    Kind::Dynamics => "dynamics",
                    Kind::Eq => "eq",
                    Kind::Filter => "filter",
                    Kind::Modulation => "modulation",
                    Kind::Delay => "delay",
                    Kind::Reverb => "reverb",
                },
            }
        }
    };
    term(key).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(ids: &[&str]) -> Board {
        Board::new(ids.iter().map(|id| Block::of(id).unwrap()).collect())
    }

    #[test]
    fn a_rig_reads_as_one_plain_line() {
        let s = chain_words(&board(&["green", "amp_crunch", "cab_4x12"]));
        let amp = if catalog::SHOW_BRANDS {
            "Marshall-style amp"
        } else {
            "British crunch amp"
        };
        assert_eq!(
            s,
            format!("Overdrive pedal \u{2192} {amp} \u{2192} 4x12 speaker box")
        );
        // off units aren't heard, so they aren't said
        let mut b = board(&["gate", "amp_clean", "cab_2x12", "spring"]);
        b.blocks[3].on = false;
        assert!(chain_words(&b).starts_with("Noise gate \u{2192} "));
        assert!(!chain_words(&b).contains("reverb"));
        assert_eq!(
            chain_words(&Board::default()),
            "Nothing on: you hear yourself dry"
        );
    }

    #[test]
    fn every_model_has_plain_words_and_every_kind_a_what_is() {
        for (m, d) in MODELS.iter().enumerate() {
            let w = model_words(d.id);
            assert!(!w.is_empty() && w != "unit", "{}", d.id);
            assert!(!what_is(Entry::Model(m)).is_empty(), "{}", d.id);
        }
        // no brand in the shared build's words
        if !catalog::SHOW_BRANDS {
            for d in MODELS {
                assert!(!model_words(d.id).contains("Marshall"), "{}", d.id);
            }
        }
    }

    #[test]
    fn the_glossary_knows_the_rooms_words() {
        for w in [
            "cab",
            "Cabinet",
            "IR",
            "capture",
            "gate",
            "Noise gate",
            "scene",
            "A/B",
            "DI",
            "mic",
            "modeller",
            "dry",
            "input boost",
            "calibrate",
            "threshold",
            "presence",
            "master",
            "blend",
            "gain ladder",
            "NAM",
        ] {
            assert!(term(w).is_some(), "{w}");
        }
        assert!(term("cab").unwrap().contains("speaker box"));
        // RS, never the game's full name (D149); one line each
        for (k, l) in GLOSSARY {
            assert!(!l.contains("Rocksmith") && !l.contains('\n'), "{k}");
        }
    }
}
