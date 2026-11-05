//! The gear info behind the amp room's "i" dot (P37.14): for every catalogue model the real unit it's inspired by,
//! its era, what the real unit is and how it works, its sound, famous players and records, and a line per front
//! control of our model. New code (no Python source). Keyed by the catalogue's model ids and knob keys (D26).
//!
//! Brand names (D42, D58): only `real` carries them, and `inspired_by` is `real` only in the `brands` build
//! (`catalog::SHOW_BRANDS`), else `inspired_by_generic`. Every other text is brand-free so every build can show it
//! (a test checks them against `BRANDS`).

use qrs_audio::board::catalog;

/// What the "i" dot says about a model.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // (`real` and `inspired_by_generic` are read only by the tests)
pub struct Info {
    pub id: &'static str,
    /// What the build may show as the real unit: "Marshall 1959 Super Lead (Plexi)" in the `brands` build, the
    /// generic line otherwise. Safe to show as is.
    pub inspired_by: &'static str,
    /// The real unit without a brand ("a classic 100-watt British plexi head").
    pub inspired_by_generic: &'static str,
    /// The real unit with its brand, whatever the build (tests and the brands build only; show `inspired_by`).
    pub real: &'static str,
    /// When the real unit came out, or the era it defines.
    pub era: &'static str,
    /// What the real unit is, how it works and why it matters, in 2-4 sentences.
    pub about: &'static str,
    /// Its sound in one line.
    pub character: &'static str,
    /// Famous players and records known for it (may be empty where nothing is well established).
    pub players: &'static [&'static str],
    /// One short line per front control of our model: (knob key, what it does).
    pub controls: &'static [(&'static str, &'static str)],
}

impl Info {
    /// "inspired by ..." as the build may show it.
    pub fn inspired_line(&self) -> String {
        format!("inspired by {}", self.inspired_by)
    }

    /// The line for a control of our model.
    pub fn control(&self, key: &str) -> Option<&'static str> {
        self.controls.iter().find(|(k, _)| *k == key).map(|(_, t)| *t)
    }
}

/// The info for a model id.
pub fn info(id: &str) -> Option<&'static Info> {
    INFO.iter().find(|i| i.id == id)
}

macro_rules! info {
    (
        id: $id:expr,
        real: $real:expr,
        generic: $gen:expr,
        era: $era:expr,
        about: $about:expr,
        character: $ch:expr,
        players: $pl:expr,
        controls: $c:expr $(,)?
    ) => {
        Info {
            id: $id,
            inspired_by: if catalog::SHOW_BRANDS { $real } else { $gen },
            inspired_by_generic: $gen,
            real: $real,
            era: $era,
            about: $about,
            character: $ch,
            players: $pl,
            controls: $c,
        }
    };
}

// -- shared controls ----------------------------------------------------------------------------------------------

type C = (&'static str, &'static str);

const BLEND: C = (
    "blend",
    "How much of the effect against your dry signal; lower keeps a bass's clean punch.",
);
const CLEAN_LOW: C = (
    "clean_low",
    "Keeps everything below this frequency clean, whatever the Blend.",
);

const MIC: C = (
    "mic",
    "Where the mic points: centre of the cone is bright, towards the edge darker.",
);
const DISTANCE: C = (
    "distance",
    "How far the mic is from the grille: close is punchy and big, far thinner and airy.",
);
const MIC_TYPE: C = (
    "mic_type",
    "Dynamic (punchy), ribbon (smooth and dark) or condenser (detailed, more room).",
);
const LOW_CUT: C = ("low_cut", "Trims rumble and boom below this frequency.");
const HIGH_CUT: C = ("high_cut", "Trims fizz above this frequency.");
const HORN: C = (
    "horn",
    "The cab's tweeter: adds the click and string zing above the cones.",
);
const DI: C = (
    "di",
    "Blends in the clean direct signal, as studios record bass: DI plus mic.",
);

const MIC_MODEL: C = (
    "mic_model",
    "A named mic's voice in place of the generic type: a 57's bite, a 121 ribbon's warmth, a 421's fuller mids, an 87's      open top.",
);

const CAB: &[C] = &[MIC, DISTANCE, MIC_TYPE, LOW_CUT, HIGH_CUT, MIC_MODEL];
const BASS_CAB: &[C] = &[MIC, DISTANCE, MIC_TYPE, LOW_CUT, HIGH_CUT, HORN, DI, MIC_MODEL];

const MASTER: C = (
    "master",
    "The output level; gain sets the drive, this sets how loud.",
);
const SYNC: C = (
    "sync",
    "Off uses the Time knob; a note value locks the delay to the song's tempo.",
);
const MIX: C = ("mix", "How much of the effect against the dry signal.");
const EQ_LEVEL: C = (
    "level",
    "Output level, to match the EQ on and off, or as a solo boost.",
);

const fn band(key: &'static str, text: &'static str) -> C {
    (key, text)
}

/// Every model's info, in catalogue order.
pub static INFO: &[Info] = &[
    // -- dynamics -------------------------------------------------------------------------------------------------
    info! {
        id: "squeeze",
        real: "MXR Dyna Comp",
        generic: "a classic 70s orange stompbox compressor",
        era: "1972",
        about: "One of the first compressors in a stompbox, from a small Rochester, New York, pedal maker. An OTA \
                chip rides the gain: it turns the pick attack down and the fading note up, so every note comes out \
                about the same level and rings longer. That squashed, slightly clicky snap became the sound of \
                country chicken-picking and clean funk and pop.",
        character: "Squashed, glassy and sustaining, with a soft click on the attack.",
        players: &["Lowell George (Little Feat)", "David Gilmour (late-70s rig)"],
        controls: &[
            ("sustain", "How hard it squeezes: more sustain and evenness, but more noise."),
            ("attack", "How much pick click gets through before the squeeze."),
            ("level", "Output level; match it to the bypassed sound."),
        ],
    },
    info! {
        id: "studio_comp",
        real: "UREI 1176 Limiting Amplifier",
        generic: "a classic late-60s studio FET rack compressor",
        era: "1967",
        about: "A rack compressor designed by the studio pioneer Bill Putnam that uses a field-effect transistor as \
                its gain cell, which makes it extremely fast: its attack can be well under a millisecond. Ratios of \
                4, 8, 12 and 20 to 1, and engineers' trick of pressing all four ratio buttons at once for a \
                savage, pumping squash. It is one of the most used compressors on rock and pop vocals, drums, bass \
                and guitar.",
        character: "Fast, punchy and forward; aggressive when pushed.",
        players: &[],
        controls: &[
            ("threshold", "The level above which it starts to compress."),
            ("ratio", "How strongly it compresses above the threshold; high is limiting."),
            ("attack", "How fast it grabs a note: fast tames peaks, slower lets the pick punch through."),
            ("release", "How fast it lets go: fast is loud and lively, slow smooth."),
            ("makeup", "Gain after the compressor, to win back the level it took."),
        ],
    },
    info! {
        id: "bass_comp",
        real: "Darkglass Super Symmetry (as a two-band bass compressor)",
        generic: "a modern two-band bass compressor pedal",
        era: "2010s",
        about: "Modern bass compressors from the Finnish maker of heavy bass preamps, made to keep a bass's level \
                even without dulling it. Ours splits the signal at a crossover and compresses the lows apart from \
                the highs, as multiband bass compressors (and RS's own) do, so a big low note doesn't duck \
                the pick and string detail above it.",
        character: "Even and solid low end, with the attack left clear.",
        players: &[],
        controls: &[
            ("compress", "How much it compresses both bands."),
            ("crossover", "Where the lows end and the highs begin."),
            ("speed", "How fast it reacts and lets go."),
            ("level", "Output level."),
        ],
    },
    info! {
        id: "gate",
        real: "Boss NS-2 Noise Suppressor",
        generic: "a late-80s compact noise gate",
        era: "1987",
        about: "A compact noise gate: it watches the guitar's level and fades the signal out when it drops below the \
                threshold, so the hiss and hum of a high-gain rig stop between notes. Its send and return let it \
                watch the clean guitar while gating the noisy pedals or amp preamp after it. It became the standard \
                gate on high-gain pedalboards.",
        character: "Silence between notes; tight, clipped stops on chugs.",
        players: &[],
        controls: &[
            ("threshold", "The level below which it closes; raise it until the hiss stops."),
            ("release", "How fast it closes once you stop: short is tight, long keeps note tails."),
            ("range", "How far it turns down when closed: about -30 dB is gentle, Full is silence."),
        ],
    },
    info! {
        id: "boost",
        real: "Xotic EP Booster (after the Maestro Echoplex EP-3 preamp)",
        generic: "a modern clean boost after a 70s tape-echo preamp",
        era: "2009 (the tape echo it copies: about 1970)",
        about: "A small clean boost that recreates the preamp of a 70s tape echo, which players left in their chain \
                even with the echo off because it made the amp sound fuller and brighter. It uses a discrete FET \
                stage with up to about 20 dB of gain, and internal switches for more treble or bass. It is used as \
                an always-on sweetener or to push an amp into breakup.",
        character: "A bigger, slightly brighter, fuller version of your amp.",
        players: &["Eddie Van Halen (the tape echo's preamp, early records)", "Jimmy Page (70s live rig)"],
        controls: &[
            ("gain", "How much it boosts, in dB; more pushes the amp into breakup."),
            ("bright", "How much treble lift it adds."),
        ],
    },
    info! {
        id: "opto_comp",
        real: "Teletronix LA-2A (and the Ampeg Opto Comp)",
        generic: "an early-60s American light-cell studio compressor",
        era: "early 1960s",
        about: "A tube studio leveller whose gain cell is a light: an electroluminescent panel glows with the signal \
                and a photocell next to it turns the gain down. The cell responds slowly and lets go in two stages, \
                fast and then slow, so it levels a performance smoothly and musically rather than grabbing each \
                peak. It has only two main knobs (gain and peak reduction) and is a classic on vocals and bass.",
        character: "Smooth, warm and gentle; it evens a part without squashing it.",
        players: &[],
        controls: &[
            ("peak", "Peak reduction: how much it compresses."),
            ("gain", "Output gain after the compressor."),
            ("mode", "Compress is gentle; Limit is firmer, holding peaks down."),
            ("mix", "Dry against compressed, for parallel compression."),
        ],
    },
    // -- drives ---------------------------------------------------------------------------------------------------
    info! {
        id: "green",
        real: "Ibanez TS808 Tube Screamer",
        generic: "a late-70s Japanese green overdrive",
        era: "1979",
        about: "A green Japanese overdrive built around a small dual op-amp, with clipping diodes in its feedback \
                loop for a soft, amp-like clip. It trims the lows before clipping and peaks the mids, which keeps \
                chords tight and makes leads cut through. Used both as a drive on a clean amp and as a boost in \
                front of a high-gain amp, it is one of the most copied pedals ever.",
        character: "Warm, smooth and mid-forward; tight in the lows.",
        players: &["Stevie Ray Vaughan", "John Mayer"],
        controls: &[
            ("drive", "How much it overdrives."),
            ("tone", "Darker to brighter."),
            ("level", "Output level; high with low drive is the classic boost."),
            BLEND,
            CLEAN_LOW,
        ],
    },
    info! {
        id: "blues",
        real: "Boss BD-2 Blues Driver",
        generic: "a mid-90s blue compact overdrive",
        era: "1995",
        about: "A blue compact overdrive made to behave like a small tube amp breaking up rather than like a classic \
                mid-boost pedal. It uses discrete transistor stages instead of an op-amp, so it follows picking \
                strength and the guitar's volume knob closely, from clean to crunchy. It has a fuller low end and \
                more treble than the green overdrives.",
        character: "Open, dynamic, amp-like breakup with bite.",
        players: &[],
        controls: &[
            ("gain", "From a light edge to a thick crunch."),
            ("tone", "Darker to brighter; it can get very bright."),
            ("level", "Output level."),
            BLEND,
            CLEAN_LOW,
        ],
    },
    info! {
        id: "gold",
        real: "Klon Centaur",
        generic: "a mid-90s boutique gold overdrive",
        era: "1994-2008 (about 8,000 made by hand)",
        about: "A hand-built gold overdrive from a Boston engineer, made to sound like a good amp turned up, only \
                more so. Inside, a charge pump runs it at 18 volts for headroom, germanium diodes do the clipping, \
                and a clean path is blended back in as the gain goes up, so the pick attack and lows survive. Its \
                legend and second-hand price spawned hundreds of copies.",
        character: "Transparent, full and open; more of your amp rather than a new sound.",
        players: &["Jeff Beck", "Joe Bonamassa"],
        controls: &[
            ("gain", "Drive, and with it less of the clean path."),
            ("treble", "An active treble control, cut or lift."),
            ("output", "Output level; it has plenty, for a clean boost."),
            BLEND,
            CLEAN_LOW,
        ],
    },
    info! {
        id: "germanium",
        real: "germanium-diode overdrives (no single unit)",
        generic: "germanium-diode overdrives (no single unit)",
        era: "2000s boutique pedals, after 60s germanium parts",
        about: "Not one pedal but a family: overdrives that clip with old-style germanium diodes instead of silicon. \
                Germanium starts clipping at a lower voltage and with a softer knee, so the drive comes in gradually \
                and stays round and compressed. Boutique makers favour it for a warm, vintage-flavoured drive.",
        character: "Soft, warm and compressed; vintage rather than aggressive.",
        players: &[],
        controls: &[
            ("gain", "How hard it drives the diodes."),
            ("tone", "Darker to brighter."),
            ("level", "Output level."),
            BLEND,
            CLEAN_LOW,
        ],
    },
    info! {
        id: "treble_boost",
        real: "Dallas Rangemaster Treble Booster",
        generic: "a mid-60s British germanium treble booster",
        era: "mid-1960s",
        about: "A small box that sat on top of the amp, with a single germanium transistor and one knob. A small \
                input capacitor means it boosts the treble far more than the bass, so it pushes a dark, cleanish \
                British amp of the time into a singing, cutting overdrive. Run into a cranked amp it shaped the \
                British blues and early hard-rock lead sound.",
        character: "Bright, cutting, singing leads; thin on its own, glorious into a driven amp.",
        players: &[
            "Brian May (Queen)",
            "Tony Iommi (early Black Sabbath)",
            "Eric Clapton (the 1966 Bluesbreakers album)",
            "Rory Gallagher",
        ],
        controls: &[
            ("boost", "How hard it pushes the amp."),
            ("range", "Where the boost starts: low is full-range, high treble only."),
            BLEND,
            CLEAN_LOW,
        ],
    },
    info! {
        id: "orange",
        real: "Boss DS-1 Distortion",
        generic: "a late-70s orange compact distortion",
        era: "1978",
        about: "An orange compact distortion and one of the first pedals of the long-running compact line. A \
                transistor stage and an op-amp drive a pair of diodes into hard clipping, and a tone control with a \
                built-in mid scoop shapes it. Cheap, everywhere and tough, it was many players' first distortion and \
                stayed on pro boards for decades.",
        character: "Bright, buzzy, hard-edged distortion with scooped mids.",
        players: &["Kurt Cobain", "Joe Satriani"],
        controls: &[
            ("dist", "How much distortion."),
            ("tone", "Darker to brighter; up high it gets cutting."),
            ("level", "Output level."),
            BLEND,
            CLEAN_LOW,
        ],
    },
    info! {
        id: "rodent",
        real: "ProCo RAT",
        generic: "a late-70s American black box distortion",
        era: "1978",
        about: "A black box distortion first built by hand in a Michigan workshop. A high-gain op-amp drives a pair \
                of silicon diodes to ground for a hard, thick clip, and its Filter knob is a low-pass that darkens \
                the sound as it turns up (the opposite of most tone knobs). It covers a wide range, from a crunchy \
                overdrive to a near fuzz, and became a staple of punk, grunge and alternative rock.",
        character: "Thick, gritty, aggressive distortion; fuzz-like when cranked.",
        players: &["Jeff Beck", "David Gilmour (80s)"],
        controls: &[
            ("distortion", "From overdrive to near fuzz."),
            ("filter", "Turning up darkens: a low-pass on the top end."),
            ("volume", "Output level."),
            BLEND,
            CLEAN_LOW,
        ],
    },
    info! {
        id: "metal",
        real: "Boss MT-2 Metal Zone",
        generic: "an early-90s compact high-gain distortion",
        era: "1991",
        about: "A compact high-gain distortion with two gain stages and a three-band EQ whose mid band can be swept, \
                aimed at players without a high-gain amp. With so much gain and so much EQ range it can sound harsh \
                set badly, which made its reputation mixed, but set with care it is a tight, saturated metal tone \
                in a box.",
        character: "Saturated, compressed, high-gain; tight with the mids in.",
        players: &[],
        controls: &[
            ("gain", "How much distortion; there is a lot."),
            ("low", "Bass cut or boost, in dB."),
            ("mid", "Mids cut or boost; cutting scoops it, boosting thickens it."),
            ("high", "Treble cut or boost."),
            ("level", "Output level."),
            BLEND,
            CLEAN_LOW,
            ("mid_freq", "Where the Mid knob works, 200 Hz to 5 kHz."),
        ],
    },
    info! {
        id: "bass_drive",
        real: "Tech 21 SansAmp Bass Driver DI",
        generic: "a mid-90s bass preamp and direct box",
        era: "mid-1990s",
        about: "A bass preamp and direct box in a pedal that imitates a miked tube bass rig with analogue circuitry, \
                no tubes. Its Blend knob mixes the dry bass with the amp-like sound, and its XLR output goes straight \
                to a desk, which made it a standard way to record and play bass live without an amp. It runs from \
                clean warmth to a gritty growl.",
        character: "Warm, punchy, amp-like bass; a growl when pushed.",
        players: &[],
        controls: &[
            ("gain", "How hard the amp-like stage drives."),
            ("blend", "The dry bass against the driven sound."),
            ("tone", "Darker to brighter."),
            ("level", "Output level."),
            CLEAN_LOW,
        ],
    },
    info! {
        id: "bass_grind",
        real: "Darkglass Microtubes B7K",
        generic: "a modern Finnish bass preamp and overdrive",
        era: "early 2010s",
        about: "A bass preamp and overdrive from a small Helsinki maker that defined the modern metal and prog bass \
                sound: a blend of the clean bass with a tight, crunchy grind. It has a four-band EQ with switchable \
                mid frequencies, plus two three-way switches that shape the lows (Grunt) and highs (Attack) going \
                into the clipping. Its direct output made it a whole rig in a pedal.",
        character: "Tight, gnarly, growling grind over a clean, solid low end.",
        players: &[],
        controls: &[
            ("drive", "How much grind."),
            ("blend", "Clean bass against the grind."),
            ("level", "Output level."),
            ("bass", "Low shelf, cut or boost."),
            ("lo_mid", "Low-mid band at the Lo-mid freq."),
            ("hi_mid", "High-mid band at the Hi-mid freq."),
            ("treble", "Top-end shelf."),
            ("grunt", "Lows going into the clipping: Fat, Flat or Raw."),
            ("attack", "Highs going into the clipping: Cut, Flat or Boost."),
            ("lo_freq", "Where the low-mid band sits."),
            ("hi_freq", "Where the high-mid band sits."),
            CLEAN_LOW,
        ],
    },
    // -- fuzz -----------------------------------------------------------------------------------------------------
    info! {
        id: "round_fuzz",
        real: "Dallas Arbiter Fuzz Face",
        generic: "a mid-60s round British fuzz",
        era: "1966",
        about: "A round London-made fuzz with just two transistors, germanium at first and silicon later. It reacts \
                strongly to the guitar: full up it is a thick, hairy fuzz, and turning the guitar's volume down \
                cleans it to a glassy crunch. Its interaction with the pickups means it must come first in the \
                chain.",
        character: "Thick, warm, woolly fuzz that cleans up with the guitar's volume.",
        players: &["Jimi Hendrix", "David Gilmour (early 70s)", "Eric Johnson"],
        controls: &[("fuzz", "How much fuzz."), ("volume", "Output level."), BLEND, CLEAN_LOW],
    },
    info! {
        id: "big_pi",
        real: "Electro-Harmonix Big Muff Pi",
        generic: "a late-60s American big-box fuzz",
        era: "1969",
        about: "A New York fuzz with four transistors: an input stage, two cascaded clipping stages and an output \
                stage, with a passive tone control that scoops the mids. It sustains for ages and sounds huge \
                rather than raspy. It has been in production in many versions since, and it is the sound of a lot \
                of 70s solos and 90s alternative rock.",
        character: "Huge, smooth, sustaining wall of fuzz with scooped mids.",
        players: &[
            "David Gilmour (\"Comfortably Numb\" solo)",
            "Billy Corgan (Siamese Dream)",
            "J Mascis (Dinosaur Jr)",
            "Jack White",
        ],
        controls: &[
            ("sustain", "How much fuzz and sustain."),
            ("tone", "From bassy to bright through a mid scoop."),
            ("volume", "Output level."),
            BLEND,
            CLEAN_LOW,
        ],
    },
    info! {
        id: "bender",
        real: "Sola Sound Tone Bender MkII",
        generic: "a mid-60s British three-transistor fuzz",
        era: "1966",
        about: "A London fuzz with three germanium transistors, a step up in gain and sustain on the two-transistor \
                fuzzes before it. It has a raspy, aggressive bite with a lot of upper mids, and it was sold under \
                several names by different British firms. It is the sound of much late-60s British rock.",
        character: "Raspy, biting, aggressive germanium fuzz.",
        players: &["Jimmy Page (Led Zeppelin I)", "Mick Ronson (Ziggy Stardust era)"],
        controls: &[("attack", "How much fuzz."), ("level", "Output level."), BLEND, CLEAN_LOW],
    },
    info! {
        id: "octave_fuzz",
        real: "Tycobrahe Octavia (after Roger Mayer's Octavia)",
        generic: "a 70s octave-up fuzz",
        era: "1967 (the original, built for Hendrix); 1970s production",
        about: "An octave-up fuzz. A transformer and a pair of diodes rectify the signal, folding each wave over so \
                it plays an octave above the note, then a fuzz stage drives it. The effect is strongest on the neck \
                pickup above the 12th fret, where it rings like a second voice. The original was built by hand for \
                Jimi Hendrix by his sound engineer; a Californian firm built the version most players know.",
        character: "Ringing, ring-modulated octave-up fuzz.",
        players: &["Jimi Hendrix (\"Purple Haze\" solo)"],
        controls: &[("fuzz", "How much fuzz."), ("volume", "Output level."), BLEND, CLEAN_LOW],
    },
    info! {
        id: "bass_muff",
        real: "Electro-Harmonix Bass Big Muff Pi",
        generic: "a late-2000s American bass fuzz",
        era: "late 2000s",
        about: "The big-box fuzz revoiced for bass: the same four-transistor sustain circuit with its tone set for \
                low strings, a Dry output or blend, and a three-way bass boost switch so the low end doesn't vanish \
                under the fuzz. It gives a bass a thick, sustaining fuzz for stoner, doom and garage rock.",
        character: "Thick, woolly bass fuzz with the low end kept.",
        players: &[],
        controls: &[
            ("sustain", "How much fuzz."),
            ("tone", "Darker to brighter."),
            ("volume", "Output level."),
            ("blend", "The dry bass against the fuzz."),
            ("bass_boost", "Adds back the lows the fuzz takes out."),
            CLEAN_LOW,
        ],
    },
    // -- amps -----------------------------------------------------------------------------------------------------
    info! {
        id: "amp_clean",
        real: "Fender Twin Reverb (blackface)",
        generic: "a 60s American 2x12 clean combo",
        era: "1963 (blackface 1963-67)",
        about: "A big American 2x12 combo with four 6L6 power tubes, about 85 watts in its 60s form, spring reverb \
                and tremolo. Its huge headroom means it stays clean at stage volume, with a scooped, sparkling top \
                and a firm low end. It became the standard clean backline amp in clubs and studios.",
        character: "Loud, sparkling, scooped clean with lots of headroom.",
        players: &["Jerry Garcia", "Eric Johnson (cleans)"],
        controls: &[
            ("gain", "Preamp volume: clean almost all the way, a little edge at the top."),
            ("bass", "Low end."),
            ("mid", "Mids; the real amp scoops them."),
            ("treble", "Top end and sparkle."),
            ("bright", "Extra treble, most at low gain."),
            MASTER,
            ("reverb", "The amp's own spring reverb: up for surf and shimmer, 0 is off."),
            ("speed", "How fast the vibrato channel's tremolo pulses."),
            ("intensity", "How deep the tremolo pulses; 0 is off."),
        ],
    },
    info! {
        id: "amp_tweed",
        real: "Fender 5E3 Tweed Deluxe",
        generic: "a 50s American tweed combo",
        era: "1955-60",
        about: "A small tweed-covered 1x12 combo of the 50s: two 6V6 power tubes, a tube rectifier, about 15 watts \
                and a single tone knob. It breaks up early and sags under hard playing, so its sound goes from warm \
                and clean to a thick, hairy crunch just with the volume. Its simple circuit is one of the most \
                copied by amp builders.",
        character: "Warm, thick, raw breakup that compresses as it gets loud.",
        players: &["Neil Young (his 1959 tweed amp on most of his records)"],
        controls: &[
            ("volume", "Preamp volume, and with it how much it breaks up."),
            ("tone", "The one tone control, dark to bright."),
            MASTER,
        ],
    },
    info! {
        id: "amp_chime",
        real: "Vox AC30 Top Boost",
        generic: "an early-60s British 2x12 class A combo",
        era: "1958 (Top Boost from about 1961)",
        about: "A British 2x12 combo with four EL84 power tubes in cathode-biased class A, about 30 watts and no \
                negative feedback. The Top Boost circuit added treble and bass controls and more gain, and its \
                speakers ring with a bright, chiming upper mid. It breaks up into a jangly, harmonically rich crunch \
                that defined the British Invasion and much indie rock after.",
        character: "Chiming, jangly, bright crunch with a rich upper mid.",
        players: &["Hank Marvin (The Shadows)", "The Beatles (early 60s)", "Brian May (Queen)", "The Edge (U2)"],
        controls: &[
            ("gain", "Preamp volume; up for the jangly breakup."),
            ("treble", "Top end; the Top Boost's chime."),
            ("bass", "Low end."),
            ("cut", "A treble cut in the power amp: turn up to tame harshness."),
            MASTER,
        ],
    },
    info! {
        id: "amp_crunch",
        real: "Marshall 1959 Super Lead (Plexi)",
        generic: "a classic 100-watt British plexi head",
        era: "1965-69 (the plexiglass-panel years)",
        about: "A 100-watt British head with four EL34 power tubes (KT66 at first), no master volume, and a gold \
                plexiglass front panel that gave the era its nickname. Played loud, its power tubes break up into \
                the bright, roaring crunch of classic hard rock; at lower volumes it is cleaner than people expect. \
                With the 4x12 stack it became the shape of rock.",
        character: "Bright, roaring, open crunch; the classic-rock sound.",
        players: &[
            "Jimi Hendrix",
            "Eric Clapton (Cream)",
            "Angus Young (AC/DC)",
            "Eddie Van Halen (Van Halen I)",
        ],
        controls: &[
            ("gain", "Preamp drive; real ones had only volume, so this is how loud you'd play it."),
            ("bass", "Low end; too much gets flubby."),
            ("mid", "Mids, the heart of the crunch."),
            ("treble", "Top end and bite."),
            ("presence", "Upper treble in the power amp, for air and cut."),
            MASTER,
        ],
    },
    info! {
        id: "amp_lead",
        real: "Marshall JCM800 2203",
        generic: "an early-80s British 100-watt master-volume head",
        era: "1981",
        about: "A 100-watt British head with EL34s and a master volume, so the preamp can be driven hard at any \
                volume, and an extra gain stage over the plexi. Its tight, aggressive midrange became the hard rock \
                and metal sound of the 80s, often with a green overdrive in front for more gain.",
        character: "Tight, aggressive, mid-forward high-gain crunch.",
        players: &["Zakk Wylde", "Kerry King (Slayer)"],
        controls: &[
            ("gain", "Preamp drive."),
            ("bass", "Low end; keep it modest for tight chugs."),
            ("mid", "Mids, the heart of its bark."),
            ("treble", "Top end and bite."),
            ("presence", "Upper treble in the power amp."),
            MASTER,
        ],
    },
    info! {
        id: "amp_modern",
        real: "Mesa/Boogie Dual Rectifier",
        generic: "an early-90s American high-gain head",
        era: "early 1990s",
        about: "A Californian high-gain head with 6L6 power tubes and a switch between tube and silicon \
                rectification (the tube rectifier sags and loosens, silicon is tight). Its modern channel has deep, \
                saturated gain with a huge low end and a scooped tone. It was the defining amp of 90s alternative \
                rock and nu-metal.",
        character: "Huge, saturated, scooped high gain with a big, loose low end.",
        players: &["Mark Tremonti (Creed)", "Brad Delson (Linkin Park)"],
        controls: &[
            ("gain", "Preamp drive; there is a lot. Turned down it is a gritty crunch: the real amp's clean channel is a clean amp of its own."),
            ("bass", "Low end; it gets enormous."),
            ("mid", "Mids; back them off for the scooped sound."),
            ("treble", "Top end."),
            ("presence", "Upper treble in the power amp."),
            ("depth", "Lowest lows in the power amp, for chug thump."),
            MASTER,
            ("mode", "The channel's voicing: Raw is the least gain and the most open, Vintage smoother with more mids, Modern the full saturated sound."),
        ],
    },
    info! {
        id: "amp_bass",
        real: "Ampeg SVT",
        generic: "a late-60s American 300-watt tube bass head",
        era: "1969",
        about: "A 300-watt all-tube bass head, first used by the Rolling Stones on their 1969 American tour, \
                usually with two 8x10 cabinets. Six power tubes give it enormous headroom, a deep, round low end \
                and a growl when pushed, and it stayed the reference rock bass amp ever since. Later versions added \
                a switchable mid frequency and Ultra Lo / Ultra Hi switches.",
        character: "Deep, round, punchy tube bass with a growl when pushed.",
        players: &["The Rolling Stones (the 1969 US tour)"],
        controls: &[
            ("gain", "Preamp drive; up for the growl."),
            ("bass", "Low end."),
            ("mid", "Mids, at the Freq."),
            ("freq", "Where the mid control works."),
            ("treble", "Top end."),
            MASTER,
            ("ultra_lo", "Deeper lows and scooped mids."),
            ("ultra_hi", "More top end and string click."),
        ],
    },
    info! {
        id: "amp_b15",
        real: "Ampeg B-15 Portaflex",
        generic: "an early-60s American tube bass combo",
        era: "1960",
        about: "A small tube bass combo with a clever design: the amp head is stored upside down inside the 1x15 \
                cabinet and flipped out to play, hence \"flip-top\". Its low power (around 25-30 watts) and \
                simple bass and treble controls give a warm, round, thumpy tone that records beautifully. It is \
                the sound of 60s soul and Motown bass.",
        character: "Warm, round, woody thump; vintage soul bass.",
        players: &["James Jamerson (Motown)"],
        controls: &[
            ("volume", "Preamp volume; up high it starts to break up."),
            ("bass", "Low end."),
            ("treble", "Top end."),
            ("input", "Lo is the gentle input, Hi the hotter one."),
            MASTER,
        ],
    },
    info! {
        id: "amp_bass_ss",
        real: "Gallien-Krueger 800RB",
        generic: "an early-80s American solid-state bass head",
        era: "1982",
        about: "A solid-state bass head from California with two power amps in one box: about 300 watts for the \
                lows and 100 for the highs, with an adjustable crossover to split them. Its punchy, clear, bright \
                tone and voicing filters cut through loud bands, and its DI made it a touring staple of the 80s \
                and 90s.",
        character: "Punchy, clear, bright and modern; tight lows.",
        players: &["Flea (Red Hot Chili Peppers)"],
        controls: &[
            ("gain", "Preamp level."),
            ("bass", "Low end, cut or boost in dB."),
            ("mid", "Mids at the Freq, cut or boost."),
            ("freq", "Where the mid control works."),
            ("treble", "Top end, cut or boost."),
            ("contour", "A mid scoop for the modern slap sound."),
            MASTER,
        ],
    },
    info! {
        id: "amp_bass_tube",
        real: "Orange AD200B",
        generic: "an early-2000s British all-tube bass head",
        era: "early 2000s",
        about: "A 200-watt all-tube British bass head with four big power tubes and a short, simple signal path: \
                one channel with gain, bass, middle and treble. It is fat and warm clean and turns into a thick, \
                hairy overdrive as the gain goes up, favoured in stoner, doom and heavy rock.",
        character: "Fat, warm, growling tube bass.",
        players: &[],
        controls: &[
            ("gain", "Preamp drive, warm to hairy."),
            ("bass", "Low end."),
            ("mid", "Mids, the growl."),
            ("treble", "Top end."),
            MASTER,
        ],
    },
    // -- cabs -----------------------------------------------------------------------------------------------------
    info! {
        id: "cab_1x10",
        real: "Fender Princeton (1x10)",
        generic: "a small 60s American 1x10 combo's speaker",
        era: "1960s",
        about: "The single 10-inch speaker of a small open-back American practice combo. A small cone in a small \
                box gives a light low end and a sweet, focused upper mid. Small combos like it have been studio \
                favourites for decades because they sound big on record at low volume.",
        character: "Small, sweet and focused; light lows.",
        players: &[],
        controls: CAB,
    },
    info! {
        id: "cab_1x12",
        real: "Fender Deluxe Reverb (1x12)",
        generic: "a 60s American open-back 1x12",
        era: "1963",
        about: "The single 12-inch speaker of a mid-size open-back American combo. The open back lets sound out of \
                the rear, so the lows are looser and the sound more spacious than a closed cab. It is one of the \
                most recorded guitar sounds in country, blues and rock.",
        character: "Balanced, open and airy; a classic studio speaker.",
        players: &[],
        controls: CAB,
    },
    info! {
        id: "cab_2x12",
        real: "Fender Twin Reverb / Vox AC30 open-back 2x12",
        generic: "an open-back 2x12 combo",
        era: "late 1950s-60s",
        about: "Two 12-inch speakers in an open-back combo, as in the big American clean combo and the British \
                class A chimer. Two speakers move more air than one, and the open back keeps it wide and \
                spacious. Bright, chiming speakers suit cleans and light crunch best.",
        character: "Wide, bright and open.",
        players: &[],
        controls: CAB,
    },
    info! {
        id: "cab_4x12",
        real: "Marshall 1960A 4x12",
        generic: "a 60s British closed-back slant 4x12",
        era: "mid-1960s",
        about: "Four 12-inch speakers in a closed-back cabinet with a slanted top, half of the classic British \
                full stack. The sealed back gives a tight, punchy low end and the low-power British speakers of the \
                time a warm, mid-rich break-up. It is the cab behind most hard rock and metal.",
        character: "Big, tight and punchy; the rock-stack sound.",
        players: &[],
        controls: CAB,
    },
    info! {
        id: "cab_8x10",
        real: "Ampeg SVT-810E",
        generic: "a 70s American 8x10 bass cabinet",
        era: "1969 (with the 300-watt tube bass head)",
        about: "Eight 10-inch speakers in one huge sealed cabinet, nicknamed the fridge for its size. Made to pair \
                with the 300-watt tube bass head, it moves a lot of air, with deep yet tight lows and a strong \
                punchy midrange. It is the classic rock bass stage sound.",
        character: "Huge, punchy and deep; the rock bass wall.",
        players: &[],
        controls: BASS_CAB,
    },
    info! {
        id: "cab_1x15",
        real: "Ampeg B-15 Portaflex 1x15",
        generic: "a 60s American 1x15 bass combo's cab",
        era: "1960",
        about: "The single 15-inch speaker in the closed cabinet of the 60s flip-top tube bass combo. A big cone \
                in a sealed box gives a round, deep thump with little top end. It is the soul and Motown record \
                bass sound.",
        character: "Round, deep, thumpy and dark.",
        players: &[],
        controls: BASS_CAB,
    },
    info! {
        id: "cab_4x10",
        real: "Eden D410XST (and the Ampeg SVT-410HLF)",
        generic: "a modern 4x10 bass cab with a horn",
        era: "1990s-2000s",
        about: "Four 10-inch speakers with a horn tweeter, the modern all-round bass cab. Smaller cones are quicker \
                than 15s, so it is tight and punchy with a clear midrange, and the horn adds the string zing and \
                click that slap and pick players want.",
        character: "Tight, punchy, clear; zing from the horn.",
        players: &[],
        controls: BASS_CAB,
    },
    info! {
        id: "cab_2x10",
        real: "a ported 2x10 bass cab (no single unit)",
        generic: "a ported 2x10 bass cab (no single unit)",
        era: "1990s onwards",
        about: "Two 10-inch speakers in a ported box: the small gig cab, easy to carry and often stacked. It has \
                less low end than a 4x10 or 8x10 but a quick, focused midrange that sits well in a small band.",
        character: "Compact, focused and quick; lighter lows.",
        players: &[],
        controls: BASS_CAB,
    },
    info! {
        id: "cab_4x15",
        real: "a vintage 4x15 bass cab (no single unit)",
        generic: "a vintage 4x15 bass cab (no single unit)",
        era: "late 1960s-70s",
        about: "Four 15-inch speakers in one big box, the vintage way to fill a hall with bass. Big cones go deep \
                and roll the top off early, so it sounds huge, dark and round.",
        character: "Huge, deep, dark and round.",
        players: &[],
        controls: BASS_CAB,
    },
    // -- EQ -------------------------------------------------------------------------------------------------------
    info! {
        id: "eq5",
        real: "a 5-band guitar graphic EQ (no single unit)",
        generic: "a 5-band guitar graphic EQ (no single unit)",
        era: "1970s onwards",
        about: "A simple graphic EQ with five fixed bands spread over the guitar's range, like the five-slider \
                pedals of the 70s and RS's own EQ. Each slider boosts or cuts its band, so the sliders draw \
                the shape of the tone: a smile scoops the mids, a frown pushes them.",
        character: "Broad strokes: scoop, push or tilt the tone.",
        players: &[],
        controls: &[
            band("b63", "Low thump at 63 Hz."),
            band("b250", "Body and warmth at 250 Hz."),
            band("b750", "Mids at 750 Hz; push to cut through, cut to scoop."),
            band("b2200", "Upper mids and bite at 2.2 kHz."),
            band("b5700", "Air and fizz at 5.7 kHz."),
            EQ_LEVEL,
        ],
    },
    info! {
        id: "eq8",
        real: "Boss GE-7 Graphic Equalizer (stretched to 8 bands)",
        generic: "a compact 7-band graphic EQ, with an extra low band",
        era: "1981",
        about: "A compact pedal with seven sliders an octave apart from 100 Hz to 6.4 kHz and a level slider, in \
                production for decades. Players use it to shape a drive, scoop or push mids, or as a clean solo \
                boost with a mid hump. Ours adds a 50 Hz band below the original seven.",
        character: "Precise tone shaping in octave steps.",
        players: &[],
        controls: &[
            band("b50", "Deep lows at 50 Hz (our extra band)."),
            band("b100", "Low thump at 100 Hz."),
            band("b200", "Body at 200 Hz."),
            band("b400", "Low mids at 400 Hz; cut for less mud."),
            band("b800", "Mids at 800 Hz, the cut-through band."),
            band("b1600", "Upper mids at 1.6 kHz."),
            band("b3200", "Bite at 3.2 kHz."),
            band("b6400", "Air and fizz at 6.4 kHz."),
            EQ_LEVEL,
        ],
    },
    info! {
        id: "bass_eq8",
        real: "Boss GEB-7 Bass Equalizer (stretched to 8 bands)",
        generic: "a compact 7-band bass graphic EQ, stretched to 8 bands",
        era: "late 1980s (the first bass version), 1990s (the current one)",
        about: "The compact graphic EQ revoiced for bass, with its bands spread lower to cover the bass's range \
                and a level slider. Bass players use it to dial in a scoop for slap, lift the low mids for \
                definition, or as a clean boost. Ours spans eight bands from 30 Hz to 16 kHz.",
        character: "Bass tone shaping, from sub to string click.",
        players: &[],
        controls: &[
            band("b30", "Sub lows at 30 Hz."),
            band("b75", "Lows and weight at 75 Hz."),
            band("b185", "Warmth at 185 Hz."),
            band("b460", "Low mids at 460 Hz, the band that makes bass heard."),
            band("b1100", "Mids and growl at 1.1 kHz."),
            band("b2700", "Finger and pick attack at 2.7 kHz."),
            band("b6800", "String zing at 6.8 kHz."),
            band("b16000", "Air at 16 kHz."),
            EQ_LEVEL,
        ],
    },
    info! {
        id: "studio_eq",
        real: "a 4-band studio console parametric EQ (no single unit)",
        generic: "a 4-band studio console parametric EQ (no single unit)",
        era: "1970s consoles",
        about: "The four-band EQ of a recording console's channel strip: a low and a high band and two mid bands, \
                each with its own frequency control. Engineers use it to carve a guitar or bass into a mix, \
                cutting where it clashes and lifting where it should speak.",
        character: "Surgical or broad tone shaping, mix-engineer style.",
        players: &[],
        controls: &[
            ("bass", "Low band boost or cut."),
            ("bass_freq", "Where the low band works."),
            ("lo_mid", "Low-mid band boost or cut."),
            ("lo_freq", "Where the low-mid band works."),
            ("hi_mid", "High-mid band boost or cut."),
            ("hi_freq", "Where the high-mid band works."),
            ("treble", "High band boost or cut."),
            ("treble_freq", "Where the high band works."),
        ],
    },
    // -- filters --------------------------------------------------------------------------------------------------
    info! {
        id: "wah",
        real: "Dunlop Cry Baby wah",
        generic: "a late-60s wah pedal",
        era: "1966 (invented), on sale from 1967",
        about: "A rocker pedal that sweeps a resonant band-pass filter up and down the range, so the guitar seems \
                to say \"wah\". It was invented in 1966 by an engineer at an American organ and amp company, and \
                its inductor-based circuit has barely changed since. Rocked in time it is funk's chicken scratch; \
                parked halfway it is a nasal, honky lead tone.",
        character: "Vocal, expressive \"wah\"; honky when parked.",
        players: &[
            "Jimi Hendrix (\"Voodoo Child (Slight Return)\")",
            "Eric Clapton (\"White Room\", Cream)",
            "Slash",
            "Kirk Hammett (Metallica)",
        ],
        controls: &[
            ("position", "The rocker: heel down is dark, toe down bright."),
            ("mode", "Pedal follows the rocker; Auto sweeps it with your playing."),
            ("sens", "In Auto, how strongly your playing opens it."),
            ("speed", "In Auto, how fast it follows."),
        ],
    },
    info! {
        id: "env_filter",
        real: "Musitronics Mu-Tron III",
        generic: "an early-70s envelope filter",
        era: "1972",
        about: "An envelope filter from a small New Jersey firm: it follows how hard you play and sweeps a resonant \
                filter with it, a wah worked by your picking. Inside, the envelope lights an LED that a photocell \
                reads to move the filter, with low-pass, band-pass and high-pass modes, two ranges and up or down \
                sweeps. It is the quack of 70s funk.",
        character: "Quacky, squelchy, funky auto-wah.",
        players: &["Stevie Wonder (\"Higher Ground\", on clavinet)", "Jerry Garcia", "Bootsy Collins"],
        controls: &[
            ("sens", "How much your playing sweeps it."),
            ("res", "Resonance: how vocal and sharp the peak is."),
            ("attack", "How fast it sweeps open."),
            ("release", "How fast it falls back."),
            ("mode", "Low-pass is round and deep; Band-pass is the classic quack."),
            ("range", "Hi for guitar, Lo for bass."),
            ("direction", "Up sweeps upward on each note, Down downward."),
            MIX,
            ("boost", "Extra output level."),
        ],
    },
    info! {
        id: "lofi",
        real: "an old radio, a telephone line, a bit crusher (no single unit)",
        generic: "an old radio, a telephone line, a bit crusher (no single unit)",
        era: "telephone and radio: early 20th century; bit crushing: 1980s samplers",
        about: "A lo-fi box: a small radio's narrow, boxy band, a telephone line's thin midrange, or the gritty \
                stepped sound of early low-resolution samplers. Producers use these to make a part sound distant, \
                old or broken.",
        character: "Thin, boxy, distant or crunchy-digital.",
        players: &[],
        controls: &[
            ("type", "Radio, phone or bit crusher."),
            MIX,
            ("drive", "How hard it is pushed; more grit."),
        ],
    },
    info! {
        id: "acoustic",
        real: "Boss AC-3 Acoustic Simulator",
        generic: "a 2000s acoustic simulator pedal",
        era: "2006",
        about: "A compact pedal that makes an electric guitar sound like an acoustic, by digitally modelling the \
                body resonance and string sparkle of several acoustic types. It is handy for switching to an \
                acoustic part mid-set without changing guitars.",
        character: "Woody, bright, strummy acoustic-like tone.",
        players: &[],
        controls: &[
            ("body", "How much body resonance."),
            ("top", "How much string sparkle on top."),
            ("level", "Output level."),
        ],
    },
    // -- modulation -----------------------------------------------------------------------------------------------
    info! {
        id: "chorus",
        real: "Boss CE-2 Chorus",
        generic: "a late-70s blue compact analogue chorus",
        era: "1979",
        about: "A compact analogue chorus that grew from the chorus circuit of a famous Japanese stereo combo amp. \
                A bucket-brigade chip delays a copy of the signal by a few milliseconds and an LFO slowly bends \
                that delay, so the copy wavers in pitch against the original, like two guitars playing together. \
                Its lush, warm shimmer is all over 80s pop and rock.",
        character: "Warm, lush, shimmering, slightly seasick.",
        players: &[],
        controls: &[
            ("rate", "How fast it sweeps."),
            ("depth", "How wide it sweeps."),
            MIX,
            ("low_cut", "Keeps the lows dry below this, for bass."),
        ],
    },
    info! {
        id: "flanger",
        real: "MXR M117 Flanger",
        generic: "a mid-70s American flanger",
        era: "late 1970s",
        about: "A flanger recreates the studio trick of slowing one of two tape machines playing the same thing \
                with a thumb on the flange of its reel. This analogue pedal uses a bucket-brigade delay swept by an \
                LFO, and its regeneration control feeds the output back for the metallic, jet-plane whoosh. It is \
                the sound of late-70s and 80s hard rock.",
        character: "Swirling, metallic jet-plane whoosh.",
        players: &["Eddie Van Halen (\"Unchained\")"],
        controls: &[
            ("rate", "How fast it sweeps."),
            ("depth", "How wide it sweeps."),
            ("regen", "Feedback: up for the metallic jet sound."),
            MIX,
            ("manual", "The sweep's centre: short for a high, sizzling jet, longer for a deeper one."),
        ],
    },
    info! {
        id: "phaser",
        real: "MXR Phase 90",
        generic: "a mid-70s orange four-stage phaser",
        era: "1974",
        about: "A small orange phaser with one knob. Four transistor all-pass stages shift the phase of a copy of \
                the signal, and mixed back with the dry guitar that creates notches an LFO sweeps up and down. \
                Subtle at slow speeds, a watery warble when fast, it is on countless 70s records.",
        character: "Warm, watery, swirling sweep.",
        players: &["Eddie Van Halen (\"Eruption\", \"Atomic Punk\")"],
        controls: &[("speed", "How fast it sweeps."), ("depth", "How strong the sweep is."), MIX],
    },
    info! {
        id: "tremolo",
        real: "Boss TR-2 Tremolo (and 60s amp tremolo)",
        generic: "a tremolo, as in old amps and a 90s compact pedal",
        era: "amp tremolo: 1950s-60s; the compact pedal: 1997",
        about: "Tremolo turns the volume up and down in a steady pulse. Old American and British amps built it in, \
                with a photocell or by wobbling the power tubes' bias, and the compact pedal brought it to any amp \
                with a choice of wave shapes. A slow, soft wave is dreamy; a fast, square one chops like a \
                helicopter.",
        character: "Pulsing, throbbing volume, from shimmering to choppy.",
        players: &["Bo Diddley", "Johnny Marr (\"How Soon Is Now?\", The Smiths)"],
        controls: &[
            ("speed", "How fast it pulses."),
            ("depth", "How deep the dips are."),
            ("wave", "Sine is smooth, triangle sharper, square choppy."),
        ],
    },
    info! {
        id: "vibe",
        real: "Shin-ei Uni-Vibe",
        generic: "a late-60s Japanese photocell phaser",
        era: "1968",
        about: "A Japanese effect designed to imitate a rotating speaker. A single lamp pulses beside four \
                photocells, each of which shifts the phase of the signal by a different amount, so the sweep is \
                uneven and throbbing rather than smooth. It came with a foot pedal for the speed and a chorus or \
                vibrato switch, and it is the sound of late-60s psychedelic guitar.",
        character: "Throbbing, liquid, uneven swirl.",
        players: &["Jimi Hendrix (\"Machine Gun\")", "Robin Trower (Bridge of Sighs)", "David Gilmour (\"Breathe\")"],
        controls: &[
            ("speed", "How fast it throbs."),
            ("intensity", "How deep the throb is."),
            ("mode", "Chorus mixes with the dry guitar; Vibrato is the wet pitch wobble alone."),
        ],
    },
    info! {
        id: "rotary",
        real: "Leslie 122 rotating speaker",
        generic: "a 50s rotating-speaker cabinet",
        era: "rotating speakers from the 1940s; the 122 from the 1950s",
        about: "A big wooden cabinet with a spinning horn for the highs and a spinning drum for the lows. As they \
                turn, the sound moves towards and away from you, so it swirls in pitch and volume, slow for a \
                chorale and fast for a churning tremolo, with a speed-up and slow-down between. Built for organs, \
                it was also used on guitar.",
        character: "Swirling, three-dimensional, churning; organ-like.",
        players: &[
            "Booker T. Jones (\"Green Onions\", organ)",
            "Jimmy Smith (organ)",
            "Eric Clapton (\"Badge\", Cream)",
        ],
        controls: &[
            ("speed", "Slow (chorale) or fast (tremolo); it ramps between."),
            ("balance", "The horn against the drum."),
            ("depth", "How strong the swirl is."),
            MIX,
        ],
    },
    info! {
        id: "octaver",
        real: "Boss OC-2 Octave",
        generic: "an early-80s compact analogue octaver",
        era: "1982",
        about: "A compact analogue octaver: it tracks the note and divides it down to one and two octaves below \
                with flip-flop circuits, mixed with your dry signal. It works on single notes only, and gives a \
                guitar a bass line underneath or a bass a huge synth-like sub.",
        character: "Deep, fat, synth-like sub-octaves.",
        players: &[],
        controls: &[
            ("sub", "One octave below."),
            ("up", "One octave above."),
            ("dry", "Your own signal."),
            ("sub2", "Two octaves below."),
        ],
    },
    // -- delay ----------------------------------------------------------------------------------------------------
    info! {
        id: "analog_delay",
        real: "MXR Carbon Copy",
        generic: "a modern analogue (bucket-brigade) delay",
        era: "2008",
        about: "A modern analogue delay built on bucket-brigade chips, which pass the signal along a chain of tiny \
                capacitors, losing a little top end each pass. The repeats come back warm and darker, sitting \
                behind the guitar instead of competing with it, and a switch adds a gentle modulation.",
        character: "Warm, dark, soft repeats.",
        players: &[],
        controls: &[
            ("time", "The gap between repeats."),
            ("repeats", "How many repeats; very high self-oscillates."),
            MIX,
            ("mod", "A gentle wobble on the repeats."),
            SYNC,
        ],
    },
    info! {
        id: "tape_echo",
        real: "Roland RE-201 Space Echo",
        generic: "a mid-70s Japanese tape echo",
        era: "1974",
        about: "A tape echo: a loop of tape runs past one record head and three playback heads, and the heads \
                chosen set the rhythm of the repeats. Each pass adds tape's warmth, saturation and a slight pitch \
                wobble, and it has a spring reverb built in. Pushed to self-oscillation it is a staple of dub \
                reggae and of countless indie and ambient records.",
        character: "Warm, wobbly, saturated repeats; rhythmic with several heads.",
        players: &["Lee \"Scratch\" Perry (dub)", "Jonny Greenwood (Radiohead)"],
        controls: &[
            ("time", "The tape speed: the gap between repeats."),
            ("repeats", "How many repeats; high runs away into oscillation."),
            MIX,
            ("wow", "Tape wobble and age."),
            ("heads", "Which playback heads sound, for rhythmic patterns."),
            SYNC,
        ],
    },
    info! {
        id: "digital_delay",
        real: "Boss DD-3 Digital Delay",
        generic: "a mid-80s compact digital delay",
        era: "1986",
        about: "A compact digital delay with up to 800 ms of clean, accurate repeats. Unlike tape and analogue \
                delays, its repeats stay as bright as the original, good for rhythmic and precise echoes. It was \
                one of the most common delays on pedalboards for decades.",
        character: "Clean, clear, accurate repeats.",
        players: &[],
        controls: &[
            ("time", "The gap between repeats."),
            ("repeats", "How many repeats."),
            MIX,
            ("tone", "Darkens the repeats to sit behind the guitar."),
            SYNC,
        ],
    },
    info! {
        id: "mod_delay",
        real: "a modulated digital delay (no single unit)",
        generic: "a modulated digital delay (no single unit)",
        era: "1980s rack delays onwards",
        about: "A digital delay whose repeats are gently bent in pitch by an LFO, like the 80s studio rack delays \
                used on big lead and clean tones. The modulation makes the repeats shimmer, a delay and chorus in \
                one.",
        character: "Shimmering, lush, wide repeats.",
        players: &[],
        controls: &[
            ("time", "The gap between repeats."),
            ("repeats", "How many repeats."),
            MIX,
            ("rate", "How fast the repeats wobble."),
            ("depth", "How much they wobble."),
            SYNC,
        ],
    },
    // -- reverb ---------------------------------------------------------------------------------------------------
    info! {
        id: "spring",
        real: "Fender 6G15 Reverb unit",
        generic: "an early-60s American tube spring reverb",
        era: "1961",
        about: "A standalone tube reverb box that sends the guitar through metal springs: a transducer shakes them \
                at one end and a pickup hears them at the other. Its dwell control sets how hard the springs are \
                driven, giving the wet, splashy, drippy sound of surf guitar.",
        character: "Wet, splashy, drippy spring reverb.",
        players: &["Dick Dale (\"Misirlou\")"],
        controls: &[
            ("dwell", "How hard the springs are driven; up is more splash."),
            ("tone", "Darker to brighter reverb."),
            MIX,
        ],
    },
    info! {
        id: "plate",
        real: "EMT 140 plate reverb",
        generic: "a late-50s studio plate reverb",
        era: "1957",
        about: "A large thin steel plate hung in a frame, shaken by a driver and heard by pickups, with a damper to \
                set the decay. Studios of the 60s and 70s used it instead of a real room: it is dense and smooth, \
                with no echoes, and sits well on vocals, drums and guitar.",
        character: "Smooth, dense, bright shimmer.",
        players: &["The Beatles (the plates at their London studio)"],
        controls: &[
            ("time", "How long the reverb lasts."),
            ("tone", "Darker to brighter."),
            ("predelay", "A gap before the reverb, to keep notes clear."),
            MIX,
        ],
    },
    info! {
        id: "room",
        real: "a small studio room (no single unit)",
        generic: "a small studio room (no single unit)",
        era: "always: the sound of a real room",
        about: "The sound of a small live room, as a mic a few feet back from the amp hears it. Short and subtle, \
                it adds depth and makes a dry amp sound like it is in a real space.",
        character: "Short, natural, subtle depth.",
        players: &[],
        controls: &[("size", "How big the room is."), ("tone", "Darker to brighter."), MIX],
    },
    info! {
        id: "hall",
        real: "a concert hall (no single unit)",
        generic: "a concert hall (no single unit)",
        era: "always: the sound of a big space",
        about: "The sound of a large hall, the long, lush reverb of a big space. It suits slow leads, ambient \
                swells and big ballads.",
        character: "Long, lush, spacious.",
        players: &[],
        controls: &[
            ("time", "How long the reverb lasts."),
            ("tone", "Darker to brighter."),
            ("predelay", "A gap before the reverb, to keep notes clear."),
            MIX,
        ],
    },
    // -- captures -------------------------------------------------------------------------------------------------
    info! {
        id: "amp_nam",
        real: "a Neural Amp Modeler (NAM) capture of a real amp",
        generic: "a neural-network capture of a real amp",
        era: "2020s",
        about: "An open-source capture: a neural network listens to a real amp's input and output and learns to \
                behave like it at one setting. A capture is only as good as the recording behind it and fixes the \
                amp's knobs where they were, so ours adds tone controls after it and a gain ladder to switch \
                between captures taken at different gains.",
        character: "Whatever amp was captured, as it was set.",
        players: &[],
        controls: &[
            ("input", "Level into the capture; more drives it harder."),
            ("output", "Level out of the capture."),
            ("bass", "Low end after the capture."),
            ("mid", "Mids after the capture."),
            ("treble", "Top end after the capture."),
            ("gate", "A noise gate threshold; all the way down is off."),
            ("gain", "Which of the unit's captures plays, by the gain it was captured at."),
        ],
    },
    info! {
        id: "pedal_nam",
        real: "a Neural Amp Modeler (NAM) capture of a pedal",
        generic: "a neural-network capture of a real pedal",
        era: "2020s",
        about: "The same open-source neural-network capture as for amps, but of a drive, fuzz or preamp pedal at \
                one setting. Ours adds a blend and a clean low path around it, as bass players set the real \
                pedals.",
        character: "Whatever pedal was captured, as it was set.",
        players: &[],
        controls: &[
            ("input", "Level into the capture."),
            ("output", "Level out of the capture."),
            BLEND,
            CLEAN_LOW,
            ("gain", "Which of the unit's captures plays, by the gain it was captured at."),
        ],
    },
    info! {
        id: "cab_ir",
        real: "an impulse response of a speaker cab",
        generic: "an impulse response of a speaker cab",
        era: "2000s onwards",
        about: "An impulse response is a recording of how a real cab, speaker and mic answer a single click; \
                playing the guitar through it gives that cab's exact sound. It is how most amp modellers do their \
                cabs.",
        character: "Exactly the cab and mic that were recorded.",
        players: &[],
        controls: &[("level", "Output level."), LOW_CUT, HIGH_CUT, DI],
    },
    // (P41.2, D167: the experts' new voices)
    info! {
        id: "amp_cali",
        real: "Mesa/Boogie Mark IIC+",
        generic: "an early-80s Californian cascading-gain lead combo",
        era: "1983-85 (the Mark series from the early 70s)",
        about: "A small Californian combo that ran its preamp stages one into the next, the first cascading-gain \
                design, for a singing, sustaining lead at any volume. The tone controls sit before the gain, which \
                makes the distortion tight and mid-heavy, and a five-band graphic EQ (80, 240, 750, 2200 and 6600 Hz) \
                after it shapes the rest. Only a few thousand were made, and it is the sound of 80s thrash rhythm.",
        character: "Tight, focused, mid-heavy saturation; the scooped V of thrash with the EQ.",
        players: &["Metallica (Master of Puppets)", "John Petrucci (Dream Theater)"],
        controls: &[
            ("gain", "The lead drive: how hard the cascade is pushed."),
            ("treble", "Top end, before the gain; it also sets the bite of the distortion."),
            ("bass", "Low end before the gain; keep it low for a tight chug."),
            ("mid", "Mids into the cascade."),
            MASTER,
            ("presence", "Upper treble in the power amp."),
            ("eq80", "The graphic EQ's 80 Hz slider: the thump."),
            ("eq240", "240 Hz: the low mids' body."),
            ("eq750", "750 Hz: down for the V scoop, up for a vocal lead."),
            ("eq2200", "2.2 kHz: attack and presence."),
            ("eq6600", "6.6 kHz: sizzle and air."),
        ],
    },
    info! {
        id: "amp_snake",
        real: "Soldano SLO-100",
        generic: "a late-80s American hot-rodded lead head",
        era: "1987",
        about: "A hand-built 100-watt American head with two channels and four 6L6-type power tubes, its overdrive \
                channel a hot-rodded British circuit taken to four gain stages. It gives a thick, smooth, singing \
                saturation with a firm low end and little fizz, and set the template for the 90s high-gain heads, \
                including the one later built with Eddie Van Halen.",
        character: "Smooth, thick, singing high gain; firm lows, a vocal upper mid.",
        players: &["Eric Clapton (Journeyman era)", "Eddie Van Halen (the head it led to)"],
        controls: &[
            ("gain", "The overdrive channel's preamp gain."),
            ("bass", "Low end."),
            ("mid", "Mids, its voice."),
            ("treble", "Top end and bite."),
            MASTER,
            ("presence", "Upper treble in the power amp."),
        ],
    },
    info! {
        id: "amp_tangerine",
        real: "Orange Rockerverb 50 MkII",
        generic: "a 2000s British two-channel all-tube head",
        era: "2003 (MkII 2010); the brand from 1968",
        about: "A 50-watt British head with two EL34 power tubes, a clean and a dirty channel, and a tube-driven \
                spring reverb, from a London maker known since 1968 for its bright orange covering and picture \
                labels. Its dirty channel is thick in the low mids and darker on top than the other British \
                stacks, a furry crunch that goes to a fat, fuzz-like saturation: a favourite of stoner, doom and \
                indie players.",
        character: "Thick, furry, low-mid-heavy crunch; a dark, fat saturation.",
        players: &["Jim Root (Slipknot)"],
        controls: &[
            ("master", "The output level (the panel's Volume)."),
            ("treble", "Top end."),
            ("mid", "The low mids it's known for."),
            ("bass", "Low end."),
            ("gain", "Drive."),
        ],
    },
    info! {
        id: "amp_surrey",
        real: "Hiwatt DR103 Custom 100",
        generic: "a late-60s British 100-watt clean-headroom head",
        era: "late 1960s-70s",
        about: "A hand-wired British 100-watt head with four EL34 power tubes and a large output transformer, built \
                to military standards. Where other 100-watt heads were already crunching it stays loud and clean, \
                articulate, with firm lows and even mids; turned right up it breaks into a punchy roar. Its two \
                channels, Normal and Brilliant, are often linked and blended.",
        character: "Huge, clean, articulate and loud; a late, punchy breakup.",
        players: &["Pete Townshend (The Who, Live at Leeds)", "David Gilmour (Pink Floyd)"],
        controls: &[
            ("gain", "The Normal channel's volume: warm and full."),
            ("brilliant", "The Brilliant channel's volume: brighter, blended with the Normal."),
            ("bass", "Low end."),
            ("treble", "Top end."),
            ("mid", "Mids."),
            ("presence", "Upper treble in the power amp."),
            ("master", "The power stage's level: up for its late breakup."),
        ],
    },
    info! {
        id: "cab_4x10_open",
        real: "Fender 5F6-A Bassman 4x10 (and the Super Reverb's)",
        generic: "a late-50s American open-back 4x10 combo",
        era: "1958-60 (the 4x10 tweed); 1963 onwards (the Super Reverb's)",
        about: "Four 10-inch speakers in an open-backed combo, first a bass amp and soon a guitar favourite. Tens \
                answer quickly and run brighter than twelves, and the open back gives an airy low end, not a closed \
                cab's thump: punchy, clear and bluesy.",
        character: "Quick, bright, punchy; airy lows.",
        players: &[],
        controls: CAB,
    },
];

/// Names that may appear only in `real` (and so in `inspired_by` in the brands build): makers, trademarked model
/// names, and parts makers. Matched as whole words, case-sensitively. (TONE3000, the capture site the app uses, is
/// named on purpose.)
#[cfg(test)]
const BRANDS: &[&str] = &[
    "Fender",
    "Marshall",
    "Vox",
    "Mesa",
    "Boogie",
    "Ampeg",
    "Boss",
    "Roland",
    "Ibanez",
    "Maxon",
    "MXR",
    "Dunlop",
    "ProCo",
    "Pro Co",
    "Klon",
    "Xotic",
    "Electro-Harmonix",
    "EHX",
    "Darkglass",
    "Tech 21",
    "SansAmp",
    "Gallien",
    "Krueger",
    "GK",
    "Orange",
    "Eden",
    "Hartke",
    "Teletronix",
    "UREI",
    "Universal Audio",
    "Leslie",
    "Hammond",
    "EMT",
    "Dallas",
    "Arbiter",
    "Sola Sound",
    "Tycobrahe",
    "Musitronics",
    "Mu-Tron",
    "Shin-ei",
    "Univox",
    "Uni-Vibe",
    "Cry Baby",
    "Thomas Organ",
    "Echoplex",
    "Maestro",
    "Gibson",
    "Celestion",
    "Jensen",
    "Hiwatt",
    "Soldano",
    "Peavey",
    "Line 6",
    "Kemper",
    "Neural DSP",
    "Aguilar",
    "Markbass",
    "Tube Screamer",
    "Big Muff",
    "Fuzz Face",
    "Tone Bender",
    "Rangemaster",
    "Twin Reverb",
    "Deluxe Reverb",
    "Bassman",
    "Portaflex",
    "Princeton",
    "Dyna Comp",
    "Phase 90",
    "Space Echo",
    "Carbon Copy",
    "Metal Zone",
    "Blues Driver",
    "Octavia",
    "Centaur",
    "JCM",
    "Plexi",
    "Rectifier",
    "SVT",
    "Super Symmetry",
    "Microtubes",
    "Strat",
    "Stratocaster",
    "Telecaster",
    "Les Paul",
    "Rickenbacker",
    "Neve",
    "Pultec",
    "Lexicon",
    "Eventide",
    "Strymon",
    "TC Electronic",
    "Bogner",
    "Diezel",
    "Engl",
    "Dumble",
    "Supro",
    "Gretsch",
    "Trace Elliot",
    "Sunn",
    "Jazz Chorus",
];

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_audio::board::MODELS;

    /// Whether `text` holds `word` as a whole word (case-sensitive).
    fn has_word(text: &str, word: &str) -> bool {
        text.match_indices(word).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + word.len()..].chars().next();
            !before.is_some_and(|c| c.is_alphanumeric()) && !after.is_some_and(|c| c.is_alphanumeric())
        })
    }

    #[test]
    fn every_model_has_its_info() {
        let mut ids = std::collections::HashSet::new();
        for i in INFO {
            assert!(ids.insert(i.id), "{} twice", i.id);
            assert!(catalog::find(i.id).is_some(), "{} isn't a model", i.id);
        }
        for m in MODELS {
            let i = info(m.id).unwrap_or_else(|| panic!("no info for {}", m.id));
            assert!(!i.real.is_empty(), "{}", m.id);
            assert!(!i.inspired_by.is_empty(), "{}", m.id);
            assert!(!i.inspired_by_generic.is_empty(), "{}", m.id);
            assert!(
                !i.about.is_empty() && !i.era.is_empty() && !i.character.is_empty(),
                "{}",
                m.id
            );
            assert!(i.players.len() <= 6, "{}: too many players", m.id);
        }
    }

    #[test]
    fn controls_name_the_models_front_knobs() {
        for m in MODELS {
            let i = info(m.id).unwrap();
            let mut seen = std::collections::HashSet::new();
            for (k, t) in i.controls {
                assert!(seen.insert(*k), "{}: {k} twice", m.id);
                assert!(!t.is_empty(), "{}: {k}", m.id);
                let kd = m.knob_index(k).map(|x| &m.knobs[x]);
                assert!(kd.is_some_and(|kd| !kd.back), "{}: {k} is no front knob", m.id);
            }
            for kd in m.knobs.iter().filter(|k| !k.back) {
                assert!(i.control(kd.key).is_some(), "{}: no line for {}", m.id, kd.key);
            }
        }
    }

    #[test]
    fn inspired_by_follows_the_brands_build() {
        for i in INFO {
            let want = if catalog::SHOW_BRANDS {
                i.real
            } else {
                i.inspired_by_generic
            };
            assert_eq!(i.inspired_by, want, "{}", i.id);
        }
    }

    #[test]
    fn only_the_real_name_carries_a_brand() {
        let mut bad = Vec::new();
        for i in INFO {
            let mut texts = vec![i.inspired_by_generic, i.era, i.about, i.character];
            texts.extend(i.players.iter().copied());
            texts.extend(i.controls.iter().map(|(_, t)| *t));
            for t in texts {
                for b in BRANDS {
                    if has_word(t, b) {
                        bad.push(format!("{}: \"{b}\" in \"{t}\"", i.id));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "brand names outside `real`:\n{}", bad.join("\n"));
        assert!(has_word("a Fender amp", "Fender") && !has_word("orange box", "Orange"));
    }

    #[test]
    fn the_older_cards_plain_lines_are_brand_free_too() {
        let mut bad = Vec::new();
        for a in super::super::data::ABOUT {
            for t in [a.plain, a.facts, a.what] {
                for b in BRANDS {
                    if has_word(t, b) {
                        bad.push(format!("{}: \"{b}\" in \"{t}\"", a.id));
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "brand names in the About cards' public lines:\n{}",
            bad.join("\n")
        );
    }
}
