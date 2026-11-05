//! The pedalboard in the app (PLAN.md Phase 14, D26): which board plays, where boards are kept, and a song's tones
//! as boards. The tile and the big view (`player/board.rs`) draw and edit them.
//!
//! - config.json: `tone_board` (your own board), `tone_boards` (boards you saved, by name), `board_follow` (follow
//!   the song's tones; on until turned off), `tone_by_input` (the tone each guitar input last had: a dry
//!   interface like the Scarlett comes up on the board, an amp modeller like the MG-300 or a microphone keeps the
//!   tone it had, "as is" rather than the board).
//! - userdata.json: `rigs` = {song id: {part: {tone key: board}}}, a song's tone as you changed it.

use qrs_audio::board::{self, Board};
use qrs_audio::devices::rank::InputKind;
use qrs_formats::Arrangement;
use qrs_library::{Config, UserData};
use serde_json::{Map, Value};

use crate::state::{AppState, AudioMode};

/// Your own board (the first starting board until you've made one).
pub fn my_board(cfg: &Config) -> Board {
    cfg.get("tone_board")
        .map(Board::from_json)
        .filter(|b| !b.blocks.is_empty())
        .unwrap_or_else(|| board::presets().swap_remove(0).1)
}

pub fn set_my_board(cfg: &mut Config, b: &Board) {
    cfg.set("tone_board", b.to_json());
}

/// The boards you saved, by name (sorted).
pub fn saved(cfg: &Config) -> Vec<(String, Board)> {
    let mut out: Vec<(String, Board)> = cfg
        .get("tone_boards")
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), Board::from_json(v))).collect())
        .unwrap_or_default();
    out.sort_by_key(|a| a.0.to_lowercase());
    out
}

pub fn save_as(cfg: &mut Config, name: &str, b: &Board) {
    let mut m = cfg
        .get("tone_boards")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    m.insert(name.to_string(), b.to_json());
    cfg.set("tone_boards", Value::Object(m));
}

pub fn delete_saved(cfg: &mut Config, name: &str) {
    if let Some(mut m) = cfg.get("tone_boards").and_then(Value::as_object).cloned() {
        m.remove(name);
        cfg.set("tone_boards", Value::Object(m));
    }
}

/// A name for a new saved board that isn't taken ("My board", "My board 2", ...).
#[allow(dead_code)] // (P37.6c: the board view's Save as moved to the amp room, D142)
pub fn fresh_name(cfg: &Config, base: &str) -> String {
    let taken: Vec<String> = saved(cfg).into_iter().map(|(n, _)| n.to_lowercase()).collect();
    if !taken.contains(&base.to_lowercase()) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !taken.contains(&n.to_lowercase()))
        .unwrap_or_else(|| base.to_string())
}

/// Your own default for a model ("Save as my default" in the editor): config.json `board_model_defaults` =
/// {model id: {knob key: value}}, the knob keys as a block's `knobs` (knobs it doesn't name keep the model's).
pub fn model_default(cfg: &Config, id: &str) -> Option<[f32; board::MAX_KNOBS]> {
    let m = cfg.get("board_model_defaults")?.get(id)?.as_object()?;
    let def = &board::MODELS[board::catalog::find(id)?];
    let mut v = def.defaults();
    for (k, x) in m {
        if let (Some(i), Some(x)) = (def.knob_index(k), x.as_f64()) {
            v[i] = def.knobs[i].clamp(x as f32);
        }
    }
    Some(v)
}

pub fn set_model_default(cfg: &mut Config, b: &board::Block) {
    let def = b.def();
    let knobs: Map<String, Value> = def
        .knobs
        .iter()
        .zip(b.v)
        .map(|(k, v)| (k.key.to_string(), Value::from(v as f64)))
        .collect();
    let mut m = cfg
        .get("board_model_defaults")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    m.insert(def.id.to_string(), Value::Object(knobs));
    cfg.set("board_model_defaults", Value::Object(m));
}

pub fn clear_model_default(cfg: &mut Config, id: &str) {
    if let Some(mut m) = cfg
        .get("board_model_defaults")
        .and_then(Value::as_object)
        .cloned()
    {
        m.remove(id);
        cfg.set("board_model_defaults", Value::Object(m));
    }
}

pub fn follow(cfg: &Config) -> bool {
    cfg.bool("board_follow", true)
}

pub fn set_follow(cfg: &mut Config, on: bool) {
    cfg.set("board_follow", on);
}

/// "Always my sound" (P41.1, the gigging guitarist's #6): your own sound plays on every song and part, not only until
/// the next one (config.json `board_always_mine`, off unless set). Following the song or holding one of its tones
/// turns it off.
pub fn always_mine(cfg: &Config) -> bool {
    cfg.bool("board_always_mine", false)
}

pub fn set_always_mine(cfg: &mut Config, on: bool) {
    cfg.set("board_always_mine", on);
}

/// Words that mark an input as an amp modeller (its own amps and effects: hear it "as is").
const MODELLERS: [&str; 14] = [
    "nux",
    "mg-300",
    "mg300",
    "helix",
    "pod",
    "headrush",
    "quad cortex",
    "katana",
    "spark",
    "gt-1",
    "gt-100",
    "boss gt",
    "kemper",
    "fractal",
];

pub fn is_modeller(name: &str) -> bool {
    let n = name.to_lowercase();
    MODELLERS.iter().any(|w| n.contains(w))
}

/// The tone an input starts on: what you last chose with it; for an input not seen before, the board for a dry
/// guitar input (`guitar`: an interface or a Real Tone Cable), and for an amp modeller, which shapes the tone
/// itself, or a microphone, the tone you had (`current`), unless that's the board ("as is" then).
pub fn tone_for_input(cfg: &Config, device_id: &str, name: &str, guitar: bool, current: &str) -> String {
    if let Some(t) = cfg
        .get("tone_by_input")
        .and_then(|m| m.get(device_id))
        .and_then(Value::as_str)
    {
        return t.to_string();
    }
    match (
        guitar && !crate::calibrate::is_modeller_input(cfg, device_id, name),
        current,
    ) {
        (true, _) => "board",
        (false, "board") => "as is",
        (false, t) => t,
    }
    .to_string()
}

pub fn remember_input_tone(cfg: &mut Config, device_id: &str, tone: &str) {
    if device_id.is_empty() {
        return;
    }
    let mut m = cfg
        .get("tone_by_input")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if m.get(device_id).and_then(Value::as_str) == Some(tone) {
        return;
    }
    m.insert(device_id.to_string(), Value::from(tone));
    cfg.set("tone_by_input", Value::Object(m));
}

/// Every frame: when the guitar input changes, put on the tone it last had (the board for a dry input, "as is"
/// for an amp modeller, when it's new); when you change the tone, remember it for this input. Returns the tone
/// put on, when it changed one.
pub fn follow_input(st: &mut AppState, seen: &mut Option<(String, String)>) -> Option<String> {
    if !matches!(st.audio_mode, AudioMode::Devices) {
        return None;
    }
    let e = st.audio.as_ref()?.engine.clone();
    let input = e.status().input?;
    let id = format!("{:?}:{}", input.device.backend, input.device.id);
    let tone = crate::tones::name_of(&e.tone()).to_string();
    match seen {
        Some((sid, stone)) if *sid == id => {
            if *stone != tone {
                remember_input_tone(&mut st.config, &id, &tone);
                st.save_config();
                *stone = tone;
            }
            None
        }
        _ => {
            let guitar = st
                .audio
                .as_ref()
                .and_then(|a| a.plan().input)
                .is_some_and(|i| matches!(i.kind, InputKind::RealToneCable | InputKind::Interface));
            let want = tone_for_input(&st.config, &id, &input.device_name, guitar, &tone);
            let changed = want != tone;
            if changed {
                let spec = crate::tones::spec_for(&st.config, &want);
                crate::tones::store(&mut st.config, &spec);
                e.set_tone(spec);
            }
            remember_input_tone(&mut st.config, &id, &want);
            st.save_config();
            crate::calibrate::apply(st); // (its trim and pluck threshold)
            *seen = Some((id, want.clone()));
            changed.then_some(want)
        }
    }
}

/// One of a song part's tones as a board.
#[derive(Clone, Debug)]
pub struct ToneSlot {
    pub key: String,
    /// A short name ("clean lead", "wah solo").
    pub label: String,
    pub board: Board,
    /// You changed it (kept in userdata.json).
    pub edited: bool,
}

/// A song part's tones as boards, and where its chart changes tone.
#[derive(Clone, Debug)]
pub struct SongRig {
    pub sid: String,
    pub part: String,
    pub tones: Vec<ToneSlot>,
    /// (chart time, tone index), in time order.
    pub changes: Vec<(f64, usize)>,
    pub base: usize,
}

fn rigs<'a>(ud: &'a UserData, sid: &str, part: &str) -> Option<&'a Map<String, Value>> {
    ud.other
        .get("rigs")
        .and_then(|r| r.get(sid))
        .and_then(|s| s.get(part.to_lowercase()))
        .and_then(Value::as_object)
}

impl SongRig {
    /// The part's tones (None when its song has no tone definitions, like most custom songs).
    pub fn new(arr: &Arrangement, sid: &str, part: &str, ud: &UserData) -> Option<SongRig> {
        if arr.rig.is_empty() {
            return None;
        }
        let kept = rigs(ud, sid, part);
        let song = crate::captures::auto::SongHint::new(&arr.artist, &arr.title);
        let mut tones: Vec<ToneSlot> = arr
            .rig
            .iter()
            .map(|t| {
                let edit = kept.and_then(|k| k.get(&t.key)).map(Board::from_json);
                let edited = edit.is_some();
                let mut board = edit.unwrap_or_else(|| crate::captures::song_board_in(t, &song));
                // (P38.2: a capture whose file is gone plays the model of ours beside it, never dry)
                crate::captures::heal(&mut board);
                ToneSlot {
                    key: t.key.clone(),
                    label: t.label(),
                    edited,
                    board,
                }
            })
            .collect();
        // two tones with the same short label get their full keys' endings told apart
        for i in 0..tones.len() {
            if tones.iter().filter(|o| o.label == tones[i].label).count() > 1 {
                tones[i].label = format!("{} {}", tones[i].label, i + 1);
            }
        }
        let index = |name: &str| tones.iter().position(|t| t.key == name || t.label == name);
        let base = index(&arr.base_tone).unwrap_or(0);
        let mut changes: Vec<(f64, usize)> = arr
            .tones
            .iter()
            .filter_map(|c| index(&c.name).map(|i| (c.time, i)))
            .collect();
        changes.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(SongRig {
            sid: sid.to_string(),
            part: part.to_string(),
            tones,
            changes,
            base,
        })
    }

    /// The tone in force at chart time `t`.
    pub fn at(&self, t: f64) -> usize {
        self.changes
            .iter()
            .take_while(|(time, _)| *time <= t + 1e-6)
            .last()
            .map(|c| c.1)
            .unwrap_or(self.base)
    }

    /// Keep your change to tone `i` (saves userdata.json).
    pub fn keep(&mut self, i: usize, board: &Board, ud: &mut UserData) {
        let Some(slot) = self.tones.get_mut(i) else {
            return;
        };
        slot.board = board.clone();
        slot.edited = true;
        let root = ud
            .other
            .entry("rigs")
            .or_insert_with(|| Value::Object(Map::new()));
        if !root.is_object() {
            *root = Value::Object(Map::new());
        }
        let song = root.as_object_mut().map(|m| {
            m.entry(self.sid.clone())
                .or_insert_with(|| Value::Object(Map::new()))
        });
        if let Some(song) = song {
            if !song.is_object() {
                *song = Value::Object(Map::new());
            }
            if let Some(sm) = song.as_object_mut() {
                let part = sm
                    .entry(self.part.to_lowercase())
                    .or_insert_with(|| Value::Object(Map::new()));
                if !part.is_object() {
                    *part = Value::Object(Map::new());
                }
                if let Some(pm) = part.as_object_mut() {
                    pm.insert(slot.key.clone(), board.to_json());
                }
            }
        }
        ud.save();
    }

    /// Back to the song's own tone `i` (forgets your change).
    pub fn reset(&mut self, i: usize, arr: &Arrangement, ud: &mut UserData) {
        let Some(slot) = self.tones.get_mut(i) else {
            return;
        };
        if let Some(t) = arr.rig.iter().find(|t| t.key == slot.key) {
            slot.board = crate::captures::song_board_in(
                t,
                &crate::captures::auto::SongHint::new(&arr.artist, &arr.title),
            );
        }
        slot.edited = false;
        let mut changed = false;
        if let Some(pm) = ud
            .other
            .get_mut("rigs")
            .and_then(|r| r.get_mut(&self.sid))
            .and_then(|s| s.get_mut(self.part.to_lowercase()))
            .and_then(Value::as_object_mut)
        {
            changed = pm.remove(&slot.key).is_some();
        }
        if changed {
            ud.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_formats::model::ToneChange;
    use qrs_formats::rig::RigTone;

    fn tone(key: &str, desc: &str, amp: &str) -> RigTone {
        let v: Value = serde_json::from_str(&format!(
            r#"{{"Key":"{key}","ToneDescriptors":["$[1]{desc}"],"GearList":{{"Amp":{{"Key":"{amp}","KnobValues":{{}}}},"Cabinet":{{"Key":"Cab_Marshall1960a_57_Edge"}}}}}}"#
        ))
        .unwrap();
        RigTone::from_json(&v).unwrap()
    }

    #[test]
    fn a_song_rig_follows_the_chart_and_keeps_edits() {
        let mut arr = Arrangement::new("Lead");
        arr.rig = vec![
            tone("Song_clean", "CLEAN", "Amp_TW22"),
            tone("Song_dist", "DISTORTION", "Amp_MarshallJCM800"),
        ];
        arr.base_tone = "Song_clean".into();
        arr.tones = vec![
            ToneChange {
                time: 30.0,
                name: "Song_dist".into(),
            },
            ToneChange {
                time: 60.0,
                name: "Song_clean".into(),
            },
        ];
        let dir = std::env::temp_dir().join(format!("qrs-rig-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut ud = UserData::load(dir.join("userdata.json"));
        let mut rig = SongRig::new(&arr, "song|key", "Lead", &ud).unwrap();
        assert_eq!(rig.tones[0].label, "clean");
        assert_eq!(rig.at(0.0), 0);
        assert_eq!(rig.at(45.0), 1);
        assert_eq!(rig.at(61.0), 0);
        // (the amp after the gentle gate a driven tone gets, P38.2)
        let amp = rig.tones[1]
            .board
            .blocks
            .iter()
            .position(|x| x.def().id == "amp_lead")
            .expect("amp_lead");
        // change the dist tone, load again: the change is kept
        let mut b = rig.tones[1].board.clone();
        b.blocks[amp].set("gain", 9.0);
        rig.keep(1, &b, &mut ud);
        let ud2 = UserData::load(dir.join("userdata.json"));
        let rig2 = SongRig::new(&arr, "song|key", "Lead", &ud2).unwrap();
        assert!(rig2.tones[1].edited);
        assert_eq!(rig2.tones[1].board.blocks[amp].get("gain"), Some(9.0));
        rig.reset(1, &arr, &mut ud);
        let ud3 = UserData::load(dir.join("userdata.json"));
        assert!(!SongRig::new(&arr, "song|key", "Lead", &ud3).unwrap().tones[1].edited);
    }

    #[test]
    fn a_models_default_is_kept_by_knob_key() {
        let dir = std::env::temp_dir().join(format!("qrs-model-default-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut cfg = Config::load(dir.join("config.json"));
        assert!(model_default(&cfg, "green").is_none());
        let mut b = board::Block::of("green").unwrap();
        b.set("drive", 2.5);
        set_model_default(&mut cfg, &b);
        let v = model_default(&cfg, "green").unwrap();
        assert_eq!(v[0], 2.5);
        assert_eq!(
            cfg.get("board_model_defaults")
                .and_then(|m| m.get("green"))
                .and_then(|g| g.get("drive")),
            Some(&Value::from(2.5))
        );
        clear_model_default(&mut cfg, "green");
        assert!(model_default(&cfg, "green").is_none());
    }

    #[test]
    fn inputs_get_the_right_tone() {
        assert!(is_modeller("NUX Audio"));
        assert!(is_modeller("Line (NUX MG-300)"));
        assert!(!is_modeller("Focusrite USB ASIO"));
        assert!(!is_modeller("Analogue 1 + 2 (Focusrite USB Audio)"));
        let dir = std::env::temp_dir().join(format!("qrs-tone-in-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut cfg = Config::load(dir.join("config.json"));
        assert_eq!(
            tone_for_input(&cfg, "asio:Focusrite", "Focusrite USB ASIO", true, "as is"),
            "board"
        );
        // a modeller, or a microphone, keeps the tone you had (but not the board)
        assert_eq!(
            tone_for_input(&cfg, "asio:NUX", "NUX Audio", true, "board"),
            "as is"
        );
        assert_eq!(
            tone_for_input(&cfg, "asio:NUX", "NUX Audio", true, "clean"),
            "clean"
        );
        assert_eq!(
            tone_for_input(&cfg, "wasapi:mic", "Microphone (C200)", false, "acoustic"),
            "acoustic"
        );
        assert_eq!(
            tone_for_input(&cfg, "wasapi:mic", "Microphone (C200)", false, "board"),
            "as is"
        );
        remember_input_tone(&mut cfg, "asio:NUX", "crunch");
        assert_eq!(
            tone_for_input(&cfg, "asio:NUX", "NUX Audio", true, "board"),
            "crunch"
        );
    }
}
