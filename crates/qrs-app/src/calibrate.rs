//! Input calibration (owner request, D33; reworked the way RS2014 does it in P23.10): each guitar input
//! gets its own trim, pluck sensitivity and kind, so the pedalboard works on anyone's interface. The boards are
//! voiced for a guitar peaking near -12 dBFS (the owner's Scarlett). Calibrate asks for the guitar's volume and tone
//! knobs all the way up, listens to two seconds with the hands off the strings (the noise floor, and any mains
//! hum), counts down, then listens to the hardest strumming you can do until its bar fills (the loudest peaks, how
//! often the interface clips, how loud the playing is). From that it recommends the trim that brings the loudest
//! playing to the boards' level with headroom left, and a pluck threshold between the noise and soft playing; it
//! says what to change on the interface when the trim can't fix it (clipping, or very quiet). The user can move
//! both afterwards; the recommendation is kept so "Back to recommended" can restore it.
//!
//! config.json `input_cal` = {input id: {trim_db, onset, kind ("auto" | "dry" | "modeller" | "mic"), peak_db,
//! floor_db, soft_db, rec_trim_db, rec_onset, instrument}}; the input id is the one `tone_by_input` uses
//! ("Asio:Focusrite USB ASIO").
//!
//! The instrument (P32.1, D89): "guitar" (the default, and what an input without the key is), "bass" or "bass5".
//! It is written only for a bass, so a guitar's entry is as it always was. An input never set takes config.json
//! `instrument` (what the first-time setup asked), else a guitar. A bass is calibrated by its own rules (P32.2):
//! plucked rather than strummed, its soft notes taken further under its playing level.

use std::time::Instant;

use qrs_analysis::instrument::Instrument;
use qrs_library::Config;
use serde_json::{Map, Value, json};

use crate::state::AppState;

/// Where calibration puts a hard strum's peaks (dBFS): what the boards are voiced for.
pub const TARGET_DB: f32 = -12.0;
/// The loudest single hit, after the trim, stays under this (dBFS): headroom for a harder hit than the test's.
pub const HEADROOM_DB: f32 = -3.0;
/// The trim's range (dB): down to -24 so a device that sends a full-scale signal still fits the boards' level
/// with headroom (P36.3; -12 before).
pub const TRIM: (f32, f32) = (-24.0, 24.0);
/// The pluck threshold's range (RMS): -56.5 .. -30.5 dBFS.
pub const ONSET: (f32, f32) = (0.0015, 0.03);
/// The pluck threshold (RMS) for a guitar input and a microphone, uncalibrated.
pub const ONSET_GUITAR: f32 = 0.004;
pub const ONSET_MIC: f32 = 0.012;
/// A soft pluck sits about this far under your hardest strumming's level (dB), and the threshold this far under that.
pub const SOFT_BELOW_DB: f32 = 20.0;
/// A bass's soft notes sit further under its playing level (P32.2): its strong fundamental keeps its level (RMS)
/// nearer its peaks than a guitar's, and a finger's soft notes and ghost notes are softer than a soft pick stroke.
pub const SOFT_BELOW_DB_BASS: f32 = 24.0;
pub const SOFT_MARGIN_DB: f32 = 6.0;
/// The threshold stays this far over the noise floor (dB).
pub const NOISE_MARGIN_DB: f32 = 10.0;
/// The hands-off listen (s; the first half second is skipped: the click of the button), the countdown, how many
/// 100 ms windows of playing fill the bar, and how long it waits for them.
pub const QUIET_S: f64 = 2.5;
const QUIET_SKIP_S: f64 = 0.5;
pub const COUNT_S: f64 = 3.0;
pub const STRUM_NEED: usize = 40;
pub const STRUM_MAX_S: f64 = 15.0;
const WINDOW_S: f64 = 0.1;

/// What an input is, as you set it (Auto: from its name and kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Auto,
    /// A dry guitar signal: the pedalboard shapes it.
    Dry,
    /// An amp modeller or anything with its own tone: heard as is, no pedalboard.
    Modeller,
    /// A microphone (in front of an acoustic guitar or an amp): heard as is, its pluck threshold over the room's
    /// noise, not heard through the speakers at first (P23.8).
    Mic,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Auto => "auto",
            Kind::Dry => "dry",
            Kind::Modeller => "modeller",
            Kind::Mic => "mic",
        }
    }

    pub fn parse(s: &str) -> Kind {
        match s {
            "dry" => Kind::Dry,
            "modeller" => Kind::Modeller,
            "mic" => Kind::Mic,
            _ => Kind::Auto,
        }
    }
}

/// One input's calibration.
#[derive(Clone, Debug, PartialEq)]
pub struct InputCal {
    pub trim_db: f32,
    /// The pluck threshold (RMS; None: the default for its kind).
    pub onset: Option<f32>,
    pub kind: Kind,
    /// What the last Calibrate measured: the loudest playing's peaks, the noise floor, soft playing's level (dBFS).
    pub peak_db: Option<f32>,
    pub floor_db: Option<f32>,
    pub soft_db: Option<f32>,
    /// What it recommended (the sliders can move away from it; "Back to recommended" restores it).
    pub rec_trim_db: Option<f32>,
    pub rec_onset: Option<f32>,
    /// What it plays (P32.1): a guitar unless set.
    pub instrument: Instrument,
    /// The last calibration's verdict (`Verdict::key`; None: an older config, read as good), and whether a verdict
    /// that asked for another go was kept anyway ("Keep these settings anyway", P36.3).
    pub verdict: Option<String>,
    pub kept: bool,
}

impl Default for InputCal {
    fn default() -> Self {
        InputCal {
            trim_db: 0.0,
            onset: None,
            kind: Kind::Auto,
            peak_db: None,
            floor_db: None,
            soft_db: None,
            rec_trim_db: None,
            rec_onset: None,
            instrument: Instrument::Guitar,
            verdict: None,
            kept: false,
        }
    }
}

impl InputCal {
    /// Whether it counts as calibrated (P36.3): measured, and the verdict didn't ask for another go (or it was
    /// kept anyway). A config from before the verdict was saved reads as good.
    pub fn calibrated(&self) -> bool {
        self.peak_db.is_some()
            && (self.kept || !self.verdict.as_deref().is_some_and(|v| Verdict::parse(v).retry()))
    }

    /// Calibrated, but with a verdict worth a word ("kept although it clipped", "a hot device").
    pub fn caveat(&self) -> Option<&'static str> {
        let v = Verdict::parse(self.verdict.as_deref()?);
        match v {
            _ if self.kept && v.retry() => Some("kept although it asked for another go"),
            Verdict::HotDevice => Some("a hot device, turned down to fit"),
            Verdict::Hot => Some("a little hot"),
            _ => None,
        }
    }

    /// Whether there's a recommendation to go back to.
    pub fn has_recommendation(&self) -> bool {
        self.rec_trim_db.is_some()
    }

    /// Whether the settings are the recommended ones (or there's none).
    pub fn is_recommended(&self) -> bool {
        let trim = self.rec_trim_db.is_none_or(|r| (r - self.trim_db).abs() < 0.05);
        let onset = match (self.rec_onset, self.onset) {
            (Some(r), Some(o)) => (db(r) - db(o)).abs() < 0.25,
            (Some(_), None) => false,
            (None, _) => true,
        };
        trim && onset
    }

    /// Back to what calibration recommended.
    pub fn restore_recommended(&mut self) {
        if let Some(t) = self.rec_trim_db {
            self.trim_db = t;
        }
        if let Some(o) = self.rec_onset {
            self.onset = Some(o);
        }
    }

    /// The pluck threshold in use (RMS).
    pub fn onset_or_default(&self, mic: bool) -> f32 {
        self.onset.unwrap_or(if mic { ONSET_MIC } else { ONSET_GUITAR })
    }
}

/// The pluck sensitivity slider: 0 (only hard plucks, the threshold at its top) .. 1 (the softest, at its bottom).
pub fn sensitivity_of(onset: f32) -> f32 {
    let (lo, hi) = (db(ONSET.0), db(ONSET.1));
    ((hi - db(onset)) / (hi - lo)).clamp(0.0, 1.0)
}

/// The threshold at a sensitivity (RMS), in half-dB steps.
pub fn onset_of(u: f32) -> f32 {
    let (lo, hi) = (db(ONSET.0), db(ONSET.1));
    let d = hi - u.clamp(0.0, 1.0) * (hi - lo);
    lin((d * 2.0).round() / 2.0).clamp(ONSET.0, ONSET.1)
}

/// The instrument an input not set plays: what the first-time setup asked (config.json `instrument`), else a guitar.
pub fn default_instrument(cfg: &Config) -> Instrument {
    Instrument::parse(cfg.get("instrument").and_then(Value::as_str).unwrap_or(""))
}

pub fn load(cfg: &Config, id: &str) -> InputCal {
    let Some(m) = cfg.get("input_cal").and_then(|m| m.get(id)) else {
        return InputCal {
            instrument: default_instrument(cfg),
            ..InputCal::default()
        };
    };
    let f = |k: &str| m.get(k).and_then(Value::as_f64).map(|v| v as f32);
    InputCal {
        trim_db: f("trim_db").unwrap_or(0.0).clamp(TRIM.0, TRIM.1),
        onset: f("onset").filter(|v| *v > 0.0),
        kind: Kind::parse(m.get("kind").and_then(Value::as_str).unwrap_or("auto")),
        peak_db: f("peak_db"),
        floor_db: f("floor_db"),
        soft_db: f("soft_db"),
        rec_trim_db: f("rec_trim_db").map(|v| v.clamp(TRIM.0, TRIM.1)),
        rec_onset: f("rec_onset").filter(|v| *v > 0.0),
        instrument: match m.get("instrument").and_then(Value::as_str) {
            Some(i) => Instrument::parse(i),
            None => default_instrument(cfg),
        },
        verdict: m.get("verdict").and_then(Value::as_str).map(str::to_string),
        kept: m.get("kept").and_then(Value::as_bool).unwrap_or(false),
    }
}

pub fn save(cfg: &mut Config, id: &str, c: &InputCal) {
    if id.is_empty() {
        return;
    }
    let mut all: Map<String, Value> = cfg
        .get("input_cal")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let r1 = |v: f32| (v * 10.0).round() / 10.0;
    let mut v = json!({"trim_db": r1(c.trim_db), "kind": c.kind.key()});
    if let Some(o) = c.onset {
        v["onset"] = json!(o);
    }
    for (k, x) in [
        ("peak_db", c.peak_db),
        ("floor_db", c.floor_db),
        ("soft_db", c.soft_db),
        ("rec_trim_db", c.rec_trim_db),
    ] {
        if let Some(x) = x {
            v[k] = json!(r1(x));
        }
    }
    if let Some(o) = c.rec_onset {
        v["rec_onset"] = json!(o);
    }
    if let Some(x) = &c.verdict {
        v["verdict"] = json!(x);
    }
    if c.kept {
        v["kept"] = json!(true);
    }
    // (a guitar's entry stays as it was, unless the setup's default is a bass: then it says guitar)
    if c.instrument.is_bass() || default_instrument(cfg).is_bass() {
        v["instrument"] = json!(c.instrument.key());
    }
    all.insert(id.to_string(), v);
    cfg.set("input_cal", Value::Object(all));
}

/// Whether an input has its own amp and effects (heard as is, no pedalboard): as you set it (an amp modeller, or a
/// microphone hearing the guitar or its amp), else by its name.
pub fn is_modeller_input(cfg: &Config, id: &str, name: &str) -> bool {
    match load(cfg, id).kind {
        Kind::Dry => false,
        Kind::Modeller | Kind::Mic => true,
        Kind::Auto => crate::pedalboard::is_modeller(name),
    }
}

/// What Auto takes an input for: a microphone by the device's kind, an amp modeller by its name, else a dry guitar.
pub fn auto_kind(st: &AppState, name: &str) -> Kind {
    let mic = st
        .audio
        .as_ref()
        .and_then(|a| a.plan().input)
        .is_some_and(|i| i.kind == qrs_audio::devices::rank::InputKind::Microphone);
    if mic {
        Kind::Mic
    } else if crate::pedalboard::is_modeller(name) {
        Kind::Modeller
    } else {
        Kind::Dry
    }
}

/// What the input now listening plays (a guitar with none).
pub fn instrument(st: &AppState) -> Instrument {
    match current_input(st) {
        Some((id, _)) => load(&st.config, &id).instrument,
        None => default_instrument(&st.config),
    }
}

/// The guitar input now listening: (its id, its name).
pub fn current_input(st: &AppState) -> Option<(String, String)> {
    let e = st.audio.as_ref()?.engine.clone();
    let input = e.status().input?;
    Some((
        format!("{:?}:{}", input.device.backend, input.device.id),
        input.device_name,
    ))
}

/// Put the current input's trim and pluck threshold on the engine (when the input changes, or its calibration).
pub fn apply(st: &AppState) {
    let Some(a) = &st.audio else { return };
    let mic = st.mic_input();
    let cal = current_input(st)
        .map(|(id, _)| load(&st.config, &id))
        .unwrap_or_default();
    a.engine.set_input_gain_db(cal.trim_db);
    a.engine.set_onset_floor(cal.onset_or_default(mic));
    a.engine.set_onset_bass(st.instrument().is_bass());
    // (P41.1, D163: the gates' threshold from this input's measured noise, after the input boost)
    qrs_audio::board::set_noise_floor(cal.floor_db.map(|f| f + cal.trim_db));
}

/// Learn the input's noise (P41.1, D163; the room's "Learn noise"): `levels` are the input's level (dBFS, after the
/// input boost) over a couple of seconds with the hands off the strings. Its median, less the boost, is kept as the
/// input's noise floor (`floor_db`, as Calibrate measures it), and the gates take their threshold from it. Returns the
/// floor after the boost, or None when nothing was heard or the input isn't known.
pub fn learn_noise(st: &mut AppState, levels: &[f32]) -> Option<f32> {
    let mut v: Vec<f32> = levels
        .iter()
        .copied()
        .filter(|l| l.is_finite() && *l > -140.0)
        .collect();
    if v.len() < 5 {
        return None;
    }
    let (id, _) = current_input(st)?;
    let median = percentile(&mut v, 50);
    let mut cal = load(&st.config, &id);
    cal.floor_db = Some(((median - cal.trim_db) * 10.0).round() / 10.0);
    save(&mut st.config, &id, &cal);
    st.save_config();
    apply(st);
    Some(median)
}

/// Whether the pedalboard tile belongs on screen: not on an amp modeller (the MG-300 has its own tone), unless you
/// put the board on it anyway.
pub fn board_tile_wanted(st: &AppState) -> bool {
    let Some((id, name)) = current_input(st) else {
        return true;
    };
    if !is_modeller_input(&st.config, &id, &name) {
        return true;
    }
    st.audio
        .as_ref()
        .is_some_and(|a| crate::tones::name_of(&a.engine.tone()) == "board")
}

pub fn db(v: f32) -> f32 {
    20.0 * v.max(1e-7).log10()
}

pub fn lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

// -- the measurement and what it recommends ----------------------------------------------------------------------

/// What a calibration heard.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measure {
    /// Hands off the strings: the level (RMS) each frame.
    pub quiet: Vec<f32>,
    /// The share of the quiet's energy at mains hum (50 or 60 Hz and their harmonics), when it was looked at.
    pub hum: Option<f32>,
    /// Strumming: the peak and the level (RMS) of each 100 ms window, and how many windows the interface clipped in.
    pub peaks: Vec<f32>,
    pub levels: Vec<f32>,
    pub clip_windows: usize,
}

/// What calibration says, in a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing heard while strumming: nothing set.
    Nothing,
    /// The interface clipped: turn its gain down and calibrate again.
    Clipping,
    /// Too quiet for the trim to bring up: turn the interface's gain up and calibrate again.
    VeryQuiet,
    /// Close to the top, or a clip or two on the hardest hits.
    Hot,
    /// A modeller (or another digital device) sending at full scale: turned down to fit, not a failure (P36.3).
    HotDevice,
    /// Mains hum while the hands were off the strings.
    Hum,
    /// A noisy input (a mic in a room, hum).
    Noisy,
    Good,
}

impl Verdict {
    /// The headline.
    pub fn head(self) -> &'static str {
        match self {
            Verdict::Nothing => "Nothing heard",
            Verdict::Clipping => "Too loud: your interface clipped",
            Verdict::VeryQuiet => "Very quiet",
            Verdict::Hot => "Calibrated, a little hot",
            Verdict::HotDevice => "Calibrated: your device sends a very hot signal",
            Verdict::Hum => "Calibrated, with some hum",
            Verdict::Noisy => "Calibrated, a noisy input",
            Verdict::Good => "Calibrated",
        }
    }

    /// Whether it asks for another go after changing something on the interface.
    pub fn retry(self) -> bool {
        matches!(self, Verdict::Nothing | Verdict::Clipping | Verdict::VeryQuiet)
    }

    /// Its word in config.json (`input_cal[id].verdict`).
    pub fn key(self) -> &'static str {
        match self {
            Verdict::Nothing => "nothing",
            Verdict::Clipping => "clipping",
            Verdict::VeryQuiet => "veryquiet",
            Verdict::Hot => "hot",
            Verdict::HotDevice => "hotdevice",
            Verdict::Hum => "hum",
            Verdict::Noisy => "noisy",
            Verdict::Good => "good",
        }
    }

    /// From its word (anything else: good, as configs before P36.3 have none).
    pub fn parse(s: &str) -> Verdict {
        [
            Verdict::Nothing,
            Verdict::Clipping,
            Verdict::VeryQuiet,
            Verdict::Hot,
            Verdict::HotDevice,
            Verdict::Hum,
            Verdict::Noisy,
        ]
        .into_iter()
        .find(|v| v.key() == s)
        .unwrap_or(Verdict::Good)
    }
}

/// The numbers a calibration found (dBFS).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// The loudest playing (the 90th percentile of the windows' peaks) and the loudest single hit.
    pub loud_db: f32,
    pub max_db: f32,
    pub floor_db: f32,
    /// Soft playing, estimated from the strumming's level.
    pub soft_db: f32,
    /// Windows with playing in them, and how many of them clipped.
    pub windows: usize,
    pub clip_windows: usize,
    pub hum: Option<f32>,
}

/// What a calibration found and what it sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// None: nothing to set (nothing was heard).
    pub cal: Option<InputCal>,
    pub verdict: Verdict,
    /// One line (the toast, the Audio page).
    pub message: String,
    /// What to do, in a sentence or two.
    pub advice: String,
    /// The setting is good as it is (no change asked of the interface).
    pub ok: bool,
    pub found: Found,
}

fn percentile(v: &mut [f32], q: usize) -> f32 {
    v.sort_by(f32::total_cmp);
    v[(v.len() * q / 100).min(v.len() - 1)]
}

/// The recommendation from what was heard; `kind` is kept.
///
/// - noise floor: the median level with the hands off;
/// - the loudest playing: the 90th percentile of the peaks of the windows with playing in them (over 6x the floor);
/// - trim: brings the loudest playing to -12 dBFS, but never the loudest single hit over -3 dBFS, -12..+24 dB in
///   half-dB steps;
/// - soft playing: 20 dB under the strumming's level (its 90th percentile); the pluck threshold 6 dB under that,
///   never within 10 dB of the noise floor, 0.0015..0.03 RMS;
/// - clipping: in 3 windows or more (or 5% of them), or the loudest playing over -1 dBFS; once or twice is "hot".
pub fn recommend(m: &Measure, kind: Kind) -> Outcome {
    recommend_for(m, kind, Instrument::Guitar)
}

/// `recommend` for an instrument: a bass's soft notes are taken `SOFT_BELOW_DB_BASS` under its playing (P32.2),
/// and the words say bass; a guitar's is `recommend`'s, unchanged.
pub fn recommend_for(m: &Measure, kind: Kind, inst: Instrument) -> Outcome {
    recommend_device(m, kind, inst, kind)
}

/// `recommend_for` knowing what the device is (`device`: the kind, Automatic resolved): on an amp modeller a
/// full-scale signal is its USB send level, not an interface clipping, so it's turned down to fit and called a
/// hot device (P36.3) instead of asking for a gain knob it hasn't got.
pub fn recommend_device(m: &Measure, kind: Kind, inst: Instrument, device: Kind) -> Outcome {
    let w = if inst.is_bass() { "bass" } else { "guitar" };
    let mut q = m.quiet.clone();
    let floor = if q.is_empty() {
        1e-5
    } else {
        percentile(&mut q, 50)
    }
    .max(1e-6);
    let gate = (floor * 6.0).max(1e-3);
    let playing: Vec<usize> = (0..m.peaks.len()).filter(|&i| m.peaks[i] > gate).collect();
    let floor_db = db(floor);
    if playing.len() < 5 {
        return Outcome {
            cal: None,
            verdict: Verdict::Nothing,
            message: format!(
                "Nothing heard while {}: is the {w} plugged in, its volume up, and the interface's instrument \
                 (INST / Hi-Z) input on?",
                if inst.is_bass() { "playing" } else { "strumming" }
            ),
            advice: format!(
                "Is the {w} plugged in, its volume up, and the interface's instrument (INST or Hi-Z) input on? \
                 Check the input channel too, then try again."
            ),
            ok: false,
            found: Found {
                floor_db,
                hum: m.hum,
                ..Found::default()
            },
        };
    }
    let mut pk: Vec<f32> = playing.iter().map(|&i| m.peaks[i]).collect();
    let loud = percentile(&mut pk, 90);
    let max = pk.last().copied().unwrap_or(loud);
    let level = if m.levels.len() == m.peaks.len() {
        let mut lv: Vec<f32> = playing.iter().map(|&i| m.levels[i]).collect();
        percentile(&mut lv, 90)
    } else {
        loud * 0.35 // (a strum's level sits about 9 dB under its peaks)
    };
    let (loud_db, max_db) = (db(loud), db(max));
    let soft_db = db(level)
        - if inst.is_bass() {
            SOFT_BELOW_DB_BASS
        } else {
            SOFT_BELOW_DB
        };
    // the trim: the loudest playing to the boards' level, with headroom over the loudest hit
    let want = TARGET_DB - loud_db;
    let cap = HEADROOM_DB - max_db;
    let mut trim = (want.min(cap).clamp(TRIM.0, TRIM.1) * 2.0).round() / 2.0;
    if trim > cap && trim > TRIM.0 {
        trim = (trim - 0.5).max(TRIM.0);
    }
    // the pluck threshold: under soft playing, over the noise
    let noise_db = floor_db + NOISE_MARGIN_DB;
    let squeezed = soft_db - SOFT_MARGIN_DB < noise_db;
    let onset = lin((soft_db - SOFT_MARGIN_DB).max(noise_db)).clamp(ONSET.0, ONSET.1);
    let cal = InputCal {
        trim_db: trim,
        onset: Some(onset),
        kind,
        peak_db: Some(loud_db),
        floor_db: Some(floor_db),
        soft_db: Some(soft_db),
        rec_trim_db: Some(trim),
        rec_onset: Some(onset),
        instrument: inst,
        verdict: None,
        kept: false,
    };
    let n = playing.len();
    let clips = m.clip_windows.min(n);
    let found = Found {
        loud_db,
        max_db,
        floor_db,
        soft_db,
        windows: n,
        clip_windows: clips,
        hum: m.hum,
    };
    let set = format!("trim {trim:+.1} dB");
    let hum = m.hum.is_some_and(|h| h > 0.5) && floor_db > -66.0;
    let clipping = clips >= 3 || clips * 20 > n || loud_db > -1.0;
    let (verdict, message, advice) = if clipping && device == Kind::Modeller {
        (
            Verdict::HotDevice,
            format!("Calibrated: your device sends a very hot signal, turned down to fit ({set})"),
            "Your modeller sends at the top of the scale over USB. quickRS turned it down to fit; if you can, \
             lower its USB (record) level in its app or menu so its loudest notes stay just under the top."
                .to_string(),
        )
    } else if clipping {
        (
            Verdict::Clipping,
            format!(
                "Your interface clipped: turn its gain knob down a little and calibrate again ({set} for now)"
            ),
            format!(
                "Turn the gain knob on your interface down a little, so its light stays off on your hardest {}, \
                 then calibrate again.",
                if inst.is_bass() { "note" } else { "strum" }
            ),
        )
    } else if want > TRIM.1 {
        (
            Verdict::VeryQuiet,
            format!(
                "Very quiet (peaks at {loud_db:.0} dB): turn the gain up on your interface and calibrate again \
                 ({set}, the most)"
            ),
            format!(
                "Turn the gain knob on your interface up (and the {w}'s volume, if it isn't all the way up), then \
                 calibrate again: the trim can add 24 dB, no more."
            ),
        )
    } else if clips > 0 || loud_db > -4.0 || max_db > -1.0 {
        (
            Verdict::Hot,
            format!(
                "A little hot (peaks at {loud_db:.0} dB): a touch less gain on the interface is safer ({set})"
            ),
            if clips > 0 {
                "It clipped once or twice on the hardest hits: a touch less gain on your interface is safer."
            } else {
                "Close to the top: a touch less gain on your interface is safer."
            }
            .to_string(),
        )
    } else if hum {
        (
            Verdict::Hum,
            format!("Calibrated, {set}; mains hum at {floor_db:.0} dB: plucks are counted above it"),
            "Some mains hum with your hands off the strings. Plug your amp or interface into the same power strip \
             as the computer; a USB-powered modeller hums through the USB ground, and a USB isolator or a ground \
             lift stops it; sit facing away from the screen. Plucks are counted above it."
                .to_string(),
        )
    } else if floor_db > -50.0 {
        (
            Verdict::Noisy,
            format!(
                "Calibrated, {set}; a noisy input (hum or room at {floor_db:.0} dB): plucks are counted above it"
            ),
            format!(
                "Hum or room noise at {floor_db:.0} dB: plucks are counted above it{}",
                if squeezed {
                    ", so very soft notes may be missed."
                } else {
                    "."
                }
            ),
        )
    } else {
        (
            Verdict::Good,
            format!("Calibrated: peaks at {loud_db:.0} dB, {set}, noise at {floor_db:.0} dB"),
            "A good level: your loudest playing has room to spare and soft notes still count.".to_string(),
        )
    };
    let mut cal = cal;
    cal.verdict = Some(verdict.key().to_string());
    Outcome {
        cal: Some(cal),
        verdict,
        message,
        advice,
        ok: matches!(verdict, Verdict::Good | Verdict::Noisy | Verdict::Hum),
        found,
    }
}

/// The calibration from the quiet part's levels (RMS) and the strumming's peak in each 100 ms window, `clipped` if
/// the interface hit full scale while strumming (D33's first form, kept for its tests; `recommend` is the full one).
#[cfg(test)]
pub fn outcome(quiet: &[f32], windows: &[f32], clipped: bool, kind: Kind) -> Outcome {
    recommend(
        &Measure {
            quiet: quiet.to_vec(),
            hum: None,
            peaks: windows.to_vec(),
            levels: Vec::new(),
            clip_windows: if clipped { windows.len() } else { 0 },
        },
        kind,
    )
}

/// The share of a stretch of signal's energy at mains hum: 50 Hz and its next two harmonics, or 60 Hz and its, the
/// larger (0..1; Goertzel at each).
pub fn hum_share(x: &[f32], rate: u32) -> f32 {
    let n = x.len();
    let energy: f64 = x.iter().map(|&s| (s as f64) * (s as f64)).sum();
    if n < 64 || rate == 0 || energy <= 1e-18 {
        return 0.0;
    }
    let power = |hz: f64| {
        let w = 2.0 * std::f64::consts::PI * hz / rate as f64;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &v in x {
            let s0 = v as f64 + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        // |X|^2, scaled so a sine's share is 1
        2.0 * (s1 * s1 + s2 * s2 - c * s1 * s2) / n as f64
    };
    let fam = |f: f64| (1..=3).map(|k| power(f * k as f64)).sum::<f64>();
    (fam(50.0).max(fam(60.0)) / energy).clamp(0.0, 1.0) as f32
}

// -- a run -------------------------------------------------------------------------------------------------------

/// Where a calibration run is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Hands off the strings.
    Quiet,
    /// 3, 2, 1.
    Count,
    /// Go: strum hard until the bar fills.
    Strum,
}

/// What a run shows now.
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub phase: Phase,
    pub head: &'static str,
    pub sub: &'static str,
    /// The countdown's number.
    pub count: Option<u32>,
    /// How far this step is (0..1).
    pub frac: f32,
}

/// A calibration in progress, fed the meters each frame.
pub struct Run {
    pub id: String,
    pub phase: Phase,
    started: Instant,
    m: Measure,
    /// When the phase began (s into the run), the gate over the noise (after the quiet), windows with playing.
    phase_at: f64,
    gate: f32,
    played: usize,
    win_start: f64,
    win_peak: f32,
    win_level: f32,
    win_clip: bool,
    clips_seen: u32,
    loudest: f32,
    kind: Kind,
    inst: Instrument,
    /// What the device is (the kind, Automatic resolved): a modeller's full-scale signal is a hot device.
    pub device: Kind,
    /// Screenshots: a made-up run, held at (its phase, how far).
    staged: Option<(Phase, f32)>,
    last_t: f64,
}

impl Run {
    pub fn new(id: String, clips: u32, kind: Kind) -> Run {
        Run::for_instrument(id, clips, kind, Instrument::Guitar)
    }

    /// A run for an instrument: a bass is asked to pluck its open strings hard rather than strum (P32.2).
    pub fn for_instrument(id: String, clips: u32, kind: Kind, inst: Instrument) -> Run {
        Run {
            id,
            phase: Phase::Quiet,
            started: Instant::now(),
            m: Measure::default(),
            phase_at: 0.0,
            gate: 1e-3,
            played: 0,
            win_start: 0.0,
            win_peak: 0.0,
            win_level: 0.0,
            win_clip: false,
            clips_seen: clips,
            loudest: 0.0,
            kind,
            inst,
            device: kind,
            staged: None,
            last_t: 0.0,
        }
    }

    /// A run held still for a screenshot.
    pub fn staged(phase: Phase, frac: f32, loudest_db: f32) -> Run {
        let mut r = Run::new(String::new(), 0, Kind::Auto);
        r.phase = phase;
        r.staged = Some((phase, frac));
        r.loudest = lin(loudest_db);
        r
    }

    pub fn is_staged(&self) -> bool {
        self.staged.is_some()
    }

    /// The loudest peak so far while strumming (0..1).
    pub fn loudest(&self) -> f32 {
        self.loudest
    }

    /// Whether the hum should be looked at now (late in the quiet, once): the caller reads the recording.
    pub fn wants_hum(&self) -> bool {
        self.staged.is_none()
            && self.phase == Phase::Quiet
            && self.m.hum.is_none()
            && self.started.elapsed().as_secs_f64() >= QUIET_S - 0.6
    }

    pub fn set_hum(&mut self, share: f32) {
        self.m.hum = Some(share);
    }

    /// What to show now.
    pub fn prompt(&self) -> Prompt {
        let (phase, frac) = match self.staged {
            Some(s) => s,
            None => (self.phase, self.frac(self.last_t)),
        };
        let since = self.last_t - self.phase_at;
        let bass = self.inst.is_bass();
        match phase {
            Phase::Quiet => Prompt {
                phase,
                head: "Hands off the strings",
                sub: "Keep still for two seconds: quickRS listens to the quiet.",
                count: None,
                frac,
            },
            Phase::Count => Prompt {
                phase,
                head: if bass {
                    "Get ready to play"
                } else {
                    "Get ready to strum"
                },
                sub: if bass {
                    "Pluck the open strings hard, one after another, until the bar fills."
                } else {
                    "Hard, on all strings, again and again, until the bar fills."
                },
                count: Some(match self.staged {
                    Some((_, f)) => (3.0 - f * 3.0).ceil().max(1.0) as u32,
                    None => (COUNT_S - since).ceil().max(1.0) as u32,
                }),
                frac,
            },
            Phase::Strum => Prompt {
                phase,
                head: if bass {
                    "Go! Pluck as hard as you can"
                } else {
                    "Go! Strum as hard as you can"
                },
                sub: if self.staged.is_none() && self.played == 0 && since > 3.0 {
                    if bass {
                        "Nothing heard yet: is the bass's volume up, and the right input and channel picked?"
                    } else {
                        "Nothing heard yet: is the guitar's volume up, and the right input and channel picked?"
                    }
                } else if bass {
                    "Every string, really hard, again and again, until the bar fills."
                } else {
                    "All strings, really loud, again and again, until the bar fills."
                },
                count: None,
                frac,
            },
        }
    }

    fn frac(&self, t: f64) -> f32 {
        match self.phase {
            Phase::Quiet => (t / QUIET_S).min(1.0) as f32,
            Phase::Count => ((t - self.phase_at) / COUNT_S).min(1.0) as f32,
            Phase::Strum => (self.played as f32 / STRUM_NEED as f32).min(1.0),
        }
    }

    /// One frame of meters (the guitar's RMS level and peak, raw, before the trim; the clip count). Returns the
    /// outcome once it's done.
    pub fn feed(&mut self, level: f32, peak: f32, clips: u32) -> Option<Outcome> {
        let t = self.started.elapsed().as_secs_f64();
        self.feed_at(t, level, peak, clips)
    }

    /// `feed` at `t` seconds into the run.
    pub fn feed_at(&mut self, t: f64, level: f32, peak: f32, clips: u32) -> Option<Outcome> {
        if self.staged.is_some() {
            return None;
        }
        self.last_t = t;
        match self.phase {
            Phase::Quiet => {
                if t > QUIET_SKIP_S {
                    self.m.quiet.push(level);
                }
                if t >= QUIET_S {
                    let mut q = self.m.quiet.clone();
                    let floor = if q.is_empty() {
                        1e-5
                    } else {
                        percentile(&mut q, 50)
                    };
                    self.gate = (floor * 6.0).max(1e-3);
                    self.phase = Phase::Count;
                    self.phase_at = t;
                }
                None
            }
            Phase::Count => {
                if t - self.phase_at >= COUNT_S {
                    self.phase = Phase::Strum;
                    self.phase_at = t;
                    self.win_start = t;
                    (self.win_peak, self.win_level, self.win_clip) = (0.0, 0.0, false);
                    self.clips_seen = clips;
                }
                None
            }
            Phase::Strum => {
                self.win_peak = self.win_peak.max(peak);
                self.win_level = self.win_level.max(level);
                if clips > self.clips_seen || peak >= 0.999 {
                    self.win_clip = true;
                }
                self.clips_seen = clips;
                if t - self.win_start >= WINDOW_S {
                    self.m.peaks.push(self.win_peak);
                    self.m.levels.push(self.win_level);
                    if self.win_peak > self.gate {
                        self.played += 1;
                        self.loudest = self.loudest.max(self.win_peak);
                        if self.win_clip {
                            self.m.clip_windows += 1;
                        }
                    }
                    (self.win_peak, self.win_level, self.win_clip) = (0.0, 0.0, false);
                    self.win_start = t;
                }
                let done = self.played >= STRUM_NEED || t - self.phase_at >= STRUM_MAX_S;
                done.then(|| recommend_device(&self.m, self.kind, self.inst, self.device))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_interface_is_trimmed_up_to_the_boards_level() {
        let quiet = vec![lin(-90.0); 20];
        let windows: Vec<f32> = (0..60)
            .map(|i| lin(if i % 3 == 0 { -70.0 } else { -30.0 }))
            .collect();
        let o = outcome(&quiet, &windows, false, Kind::Auto);
        let c = o.cal.unwrap();
        assert!((c.trim_db - 18.0).abs() < 0.6, "{c:?}");
        assert!(o.ok, "{}", o.message);
        // the pluck threshold drops for a quiet input, never below its floor
        assert!(c.onset.unwrap() < ONSET_GUITAR && c.onset.unwrap() >= 0.0015);
    }

    #[test]
    fn clipping_very_quiet_silence_and_noise_are_named() {
        let quiet = vec![lin(-90.0); 20];
        let loud: Vec<f32> = vec![lin(-0.5); 40];
        let o = outcome(&quiet, &loud, true, Kind::Dry);
        assert!(!o.ok && o.message.contains("clipped"));
        assert_eq!(o.cal.unwrap().kind, Kind::Dry);
        let faint: Vec<f32> = vec![lin(-45.0); 40];
        let o = outcome(&quiet, &faint, false, Kind::Auto);
        assert!(!o.ok && o.message.contains("Very quiet"));
        assert_eq!(o.cal.unwrap().trim_db, TRIM.1);
        let o = outcome(&quiet, &[lin(-91.0); 40], false, Kind::Auto);
        assert!(o.cal.is_none() && o.message.contains("Nothing heard"));
        // a mic in a room: the threshold rises over the noise
        let noisy = vec![lin(-40.0); 20];
        let o = outcome(&noisy, &[lin(-10.0); 40], false, Kind::Auto);
        assert!(o.ok && o.message.contains("noisy"));
        assert!(o.cal.unwrap().onset.unwrap() > ONSET_MIC * 0.9);
    }

    #[test]
    fn calibrations_are_kept_per_input_and_kind_overrides_the_name() {
        let mut cfg = Config::default();
        assert!(is_modeller_input(&cfg, "Asio:NUX Audio", "NUX Audio"));
        let mut c = load(&cfg, "Asio:NUX Audio");
        c.kind = Kind::Dry;
        c.trim_db = 6.0;
        save(&mut cfg, "Asio:NUX Audio", &c);
        assert!(!is_modeller_input(&cfg, "Asio:NUX Audio", "NUX Audio"));
        assert_eq!(load(&cfg, "Asio:NUX Audio").trim_db, 6.0);
        assert_eq!(load(&cfg, "Wasapi:other"), InputCal::default());
        let mut m = load(&cfg, "Wasapi:Line (Scarlett Solo)");
        m.kind = Kind::Modeller;
        save(&mut cfg, "Wasapi:Line (Scarlett Solo)", &m);
        assert!(is_modeller_input(
            &cfg,
            "Wasapi:Line (Scarlett Solo)",
            "Line (Scarlett Solo)"
        ));
        // a microphone is heard as is too, and its kind is kept
        let mut c = load(&cfg, "Wasapi:Mic");
        c.kind = Kind::Mic;
        save(&mut cfg, "Wasapi:Mic", &c);
        assert_eq!(load(&cfg, "Wasapi:Mic").kind, Kind::Mic);
        assert!(is_modeller_input(&cfg, "Wasapi:Mic", "Mic"));
        assert_eq!(Kind::parse(Kind::Mic.key()), Kind::Mic);
    }

    /// A strum: windows of playing at `loud` dB (every fifth a bit softer), the level 9 dB under the peaks.
    fn strum(loud: f32, n: usize) -> (Vec<f32>, Vec<f32>) {
        let pk: Vec<f32> = (0..n)
            .map(|i| lin(if i % 5 == 0 { loud - 6.0 } else { loud }))
            .collect();
        let lv = pk.iter().map(|p| p * lin(-9.0)).collect();
        (pk, lv)
    }

    fn measure(floor: f32, loud: f32, clips: usize, hum: Option<f32>) -> Measure {
        let (peaks, levels) = strum(loud, 40);
        Measure {
            quiet: vec![lin(floor); 60],
            hum,
            peaks,
            levels,
            clip_windows: clips,
        }
    }

    #[test]
    fn loud_soft_clipping_and_noisy_inputs_get_sensible_settings() {
        // a healthy interface: peaks at -15, a quiet room
        let o = recommend(&measure(-75.0, -15.0, 0, Some(0.05)), Kind::Dry);
        let c = o.cal.clone().unwrap();
        assert_eq!(o.verdict, Verdict::Good, "{}", o.message);
        assert!(o.ok);
        assert_eq!(c.trim_db, 3.0);
        // soft playing 20 dB under the strum's level (-24), the threshold 6 under that: -50 dB
        assert!((db(c.onset.unwrap()) + 50.0).abs() < 0.6, "{c:?}");
        assert!((c.soft_db.unwrap() + 44.0).abs() < 0.6);
        assert_eq!((c.rec_trim_db, c.rec_onset), (Some(c.trim_db), c.onset));
        assert!(c.is_recommended());
        // a soft interface: trimmed up a lot, the threshold low but never under its range
        let o = recommend(&measure(-90.0, -34.0, 0, None), Kind::Auto);
        let c = o.cal.unwrap();
        assert_eq!(c.trim_db, 22.0);
        assert!(c.onset.unwrap() >= ONSET.0 && c.onset.unwrap() < ONSET_GUITAR);
        // hot: the trim comes down, and the loudest hit keeps 3 dB of headroom after it
        let o = recommend(&measure(-80.0, -3.0, 0, None), Kind::Auto);
        assert_eq!(o.verdict, Verdict::Hot);
        assert!(o.found.max_db + o.cal.unwrap().trim_db <= HEADROOM_DB + 1e-3);
        // one clip on the hardest hit: hot, not "turn it down and calibrate again"
        let o = recommend(&measure(-80.0, -8.0, 1, None), Kind::Auto);
        assert_eq!(o.verdict, Verdict::Hot);
        assert!(o.advice.contains("once or twice"));
        // clipping in many windows: turn the interface down, try again
        let o = recommend(&measure(-80.0, -6.0, 12, None), Kind::Auto);
        assert_eq!(o.verdict, Verdict::Clipping);
        assert!(o.verdict.retry() && !o.ok && o.advice.contains("down"));
        // far too quiet: turn it up
        let o = recommend(&measure(-90.0, -48.0, 0, None), Kind::Auto);
        assert_eq!(o.verdict, Verdict::VeryQuiet);
        assert!(o.advice.contains("up") && o.cal.unwrap().trim_db == TRIM.1);
        // a noisy room: the threshold stays 10 dB over the noise even if soft playing is lower
        let o = recommend(&measure(-45.0, -12.0, 0, None), Kind::Mic);
        assert_eq!(o.verdict, Verdict::Noisy);
        let c = o.cal.unwrap();
        assert!(db(c.onset.unwrap()) >= -35.5, "{c:?}");
        assert!(o.advice.contains("may be missed"));
        // hum with the hands off: named, still calibrated
        let o = recommend(&measure(-60.0, -15.0, 0, Some(0.9)), Kind::Dry);
        assert_eq!(o.verdict, Verdict::Hum);
        assert!(o.ok && o.advice.contains("hum"));
        // nothing played: nothing set
        let o = recommend(&measure(-80.0, -85.0, 0, None), Kind::Dry);
        assert_eq!(o.verdict, Verdict::Nothing);
        assert!(o.cal.is_none());
    }

    #[test]
    fn slider_overrides_are_saved_restored_and_undone() {
        let mut cfg = Config::default();
        let id = "Wasapi:Line (NUX Audio)";
        let rec = recommend(&measure(-75.0, -15.0, 0, None), Kind::Dry).cal.unwrap();
        save(&mut cfg, id, &rec);
        let mut c = load(&cfg, id);
        assert!(c.is_recommended() && c.has_recommendation());
        // the sliders move away from the recommendation
        c.trim_db = 7.5;
        c.onset = Some(onset_of(0.9));
        save(&mut cfg, id, &c);
        let back = load(&cfg, id);
        assert_eq!(back.trim_db, 7.5);
        assert!((db(back.onset.unwrap()) - db(onset_of(0.9))).abs() < 0.01);
        assert!(!back.is_recommended());
        assert_eq!(back.rec_trim_db, Some(3.0));
        // Back to recommended
        let mut b = back.clone();
        b.restore_recommended();
        assert!(b.is_recommended());
        assert_eq!(b.trim_db, 3.0);
        assert!((db(b.onset.unwrap()) - db(rec.onset.unwrap())).abs() < 0.25);
        // an old calibration (no recommendation kept): nothing to go back to, and nothing differs
        let old = InputCal {
            trim_db: 4.0,
            onset: Some(0.003),
            ..InputCal::default()
        };
        assert!(!old.has_recommendation() && old.is_recommended());
        // the sensitivity slider round-trips in half-dB steps, more sensitive to the right
        for u in [0.0, 0.25, 0.5, 1.0] {
            assert!((sensitivity_of(onset_of(u)) - u).abs() < 0.02, "{u}");
        }
        assert!(onset_of(1.0) < onset_of(0.0));
        assert!((db(onset_of(0.0)) - db(ONSET.1)).abs() < 0.1);
    }

    /// P32.1 / P32.2: the instrument is kept per input (a guitar's entry has no key, as before), an input never set
    /// takes the setup's default, and a bass gets its own pluck threshold from the same playing; a guitar's
    /// calibration is what it was.
    #[test]
    fn the_instrument_is_kept_and_a_bass_is_calibrated_by_its_rules() {
        let mut cfg = Config::default();
        let id = "Asio:Focusrite USB ASIO";
        let g = recommend(&measure(-75.0, -15.0, 0, None), Kind::Dry).cal.unwrap();
        assert_eq!(g.instrument, Instrument::Guitar);
        save(&mut cfg, id, &g);
        let raw = cfg.get("input_cal").and_then(|m| m.get(id)).cloned().unwrap();
        assert!(raw.get("instrument").is_none(), "{raw}");
        assert_eq!(load(&cfg, id), g);
        // the same playing on a bass: the same trim, the threshold 4 dB lower, the words a bass's
        let m = measure(-75.0, -15.0, 0, None);
        let b = recommend_for(&m, Kind::Dry, Instrument::Bass);
        let bc = b.cal.clone().unwrap();
        assert_eq!(bc.trim_db, g.trim_db);
        assert!(
            (db(g.onset.unwrap()) - db(bc.onset.unwrap()) - 4.0).abs() < 0.6,
            "{g:?} {bc:?}"
        );
        assert_eq!(
            recommend_for(&m, Kind::Dry, Instrument::Guitar),
            recommend(&m, Kind::Dry)
        );
        let none = recommend_for(&measure(-80.0, -85.0, 0, None), Kind::Dry, Instrument::Bass5);
        assert!(none.message.contains("bass") && !none.message.contains("guitar"));
        save(&mut cfg, id, &bc);
        assert_eq!(load(&cfg, id).instrument, Instrument::Bass);
        // the setup said bass: inputs never set are basses; one set to guitar says so
        assert_eq!(load(&cfg, "Wasapi:other").instrument, Instrument::Guitar);
        cfg.set("instrument", json!("bass5"));
        assert_eq!(load(&cfg, "Wasapi:other").instrument, Instrument::Bass5);
        let mut c = load(&cfg, "Wasapi:other");
        c.instrument = Instrument::Guitar;
        save(&mut cfg, "Wasapi:other", &c);
        assert_eq!(load(&cfg, "Wasapi:other").instrument, Instrument::Guitar);
        // a bass's run asks it to pluck
        let r = Run::for_instrument(String::new(), 0, Kind::Dry, Instrument::Bass);
        assert!(r.prompt().head.contains("Hands off"));
        let mut r = r;
        let mut t = 0.0;
        while r.phase != Phase::Count {
            r.feed_at(t, lin(-80.0), lin(-70.0), 0);
            t += 0.02;
        }
        assert!(r.prompt().sub.contains("Pluck"));
    }

    #[test]
    fn hum_is_told_from_noise() {
        let rate = 48_000;
        let sine = |hz: f32| -> Vec<f32> {
            (0..rate)
                .map(|i| 0.01 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
                .collect()
        };
        assert!(hum_share(&sine(50.0), rate as u32) > 0.9);
        assert!(hum_share(&sine(120.0), rate as u32) > 0.9);
        assert!(hum_share(&sine(440.0), rate as u32) < 0.05);
        // white-ish noise (a fixed generator)
        let mut x = 12345u32;
        let noise: Vec<f32> = (0..rate)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect();
        assert!(hum_share(&noise, rate as u32) < 0.05);
        assert_eq!(hum_share(&[], rate as u32), 0.0);
    }

    #[test]
    fn a_run_listens_counts_down_and_fills_its_bar_with_playing() {
        let mut r = Run::new("Asio:X".into(), 0, Kind::Dry);
        let mut t = 0.0;
        let dt = 1.0 / 60.0;
        // quiet
        while r.phase == Phase::Quiet {
            assert!(r.feed_at(t, lin(-80.0), lin(-70.0), 0).is_none());
            t += dt;
        }
        assert_eq!(r.phase, Phase::Count);
        assert_eq!(r.prompt().count, Some(3));
        while r.phase == Phase::Count {
            assert!(r.feed_at(t, lin(-80.0), lin(-70.0), 0).is_none());
            t += dt;
        }
        // strumming that stops for two seconds: the bar waits for it
        let start = t;
        let mut out = None;
        let mut paused_frac = None;
        while out.is_none() && t - start < 20.0 {
            let playing = !(2.0..4.0).contains(&(t - start));
            let (lv, pk) = if playing {
                (lin(-24.0), lin(-15.0))
            } else {
                (lin(-80.0), lin(-70.0))
            };
            if paused_frac.is_none() && t - start > 3.9 {
                paused_frac = Some(r.prompt().frac);
            }
            out = r.feed_at(t, lv, pk, 0);
            t += dt;
        }
        let o = out.expect("done");
        // 40 windows of playing and two seconds of pause: about six seconds
        assert!((t - start - 6.0).abs() < 0.4, "{}", t - start);
        let f = paused_frac.unwrap();
        assert!((0.4..0.6).contains(&f), "{f}");
        assert_eq!(o.verdict, Verdict::Good, "{}", o.message);
        assert_eq!(o.cal.unwrap().trim_db, 3.0);
        assert!((db(r.loudest()) + 15.0).abs() < 0.1);
        // a staged run stays as it was made
        let mut s = Run::staged(Phase::Strum, 0.6, -14.0);
        assert!(s.feed_at(100.0, 0.5, 0.5, 9).is_none());
        assert_eq!(s.prompt().frac, 0.6);
    }

    #[test]
    fn a_hot_modeller_is_turned_down_not_failed() {
        // a modeller over USB peaking at full scale: the "interface clipped" in every window
        let m = measure(-80.0, -0.2, 40, None);
        let o = recommend_device(&m, Kind::Auto, Instrument::Guitar, Kind::Modeller);
        assert_eq!(o.verdict, Verdict::HotDevice);
        assert!(!o.verdict.retry());
        let c = o.cal.clone().unwrap();
        assert!(c.trim_db <= -12.0, "{}", c.trim_db);
        assert!(c.calibrated());
        assert!(o.advice.contains("USB") && !o.advice.contains("gain knob"));
        // a dry guitar with real clip windows still clips
        let o = recommend_device(&m, Kind::Dry, Instrument::Guitar, Kind::Dry);
        assert_eq!(o.verdict, Verdict::Clipping);
        assert!(o.advice.contains("gain knob"));
    }

    #[test]
    fn a_failed_calibration_is_not_calibrated_unless_kept() {
        let mut cfg = Config::default();
        let o = recommend(&measure(-80.0, -0.2, 40, None), Kind::Dry);
        assert_eq!(o.verdict, Verdict::Clipping);
        let mut c = o.cal.unwrap();
        save(&mut cfg, "W:clip", &c);
        let back = load(&cfg, "W:clip");
        assert_eq!(back.verdict.as_deref(), Some("clipping"));
        assert!(!back.calibrated());
        c.kept = true;
        save(&mut cfg, "W:clip", &c);
        let back = load(&cfg, "W:clip");
        assert!(back.kept && back.calibrated());
        assert!(back.caveat().is_some());
        // a good one, and one saved before verdicts were (read as good)
        let good = recommend(&measure(-75.0, -15.0, 0, None), Kind::Dry).cal.unwrap();
        assert!(good.calibrated() && good.caveat().is_none());
        let mut old = good.clone();
        old.verdict = None;
        save(&mut cfg, "W:old", &old);
        assert!(load(&cfg, "W:old").calibrated());
        for v in [
            Verdict::Clipping,
            Verdict::HotDevice,
            Verdict::Good,
            Verdict::VeryQuiet,
        ] {
            assert_eq!(Verdict::parse(v.key()), v);
        }
    }
}
