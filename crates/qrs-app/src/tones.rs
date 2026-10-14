//! The guitar tone in config.json, in the Python app's keys (so both apps read the same file): `tone_v2` is the
//! tone's name ("as is", "clean", "crunch", "lead", "custom"; quickRS adds "bass" and "acoustic", which the Python
//! app reads as "as is"), `tone_custom` the custom tone's knobs, `tone_presets` the saved tones by name and
//! `tone_preset` the one last chosen (port of the tone half of rsplay/settingsview.py and engine.TONES).

use qrs_audio::tone::{KNOBS, Preset, ToneParams, ToneSpec};
use qrs_library::Config;
use serde_json::{Map, Value};

/// Every tone, in the order the settings page shows them.
pub const TONES: [&str; 7] = ["as is", "clean", "crunch", "lead", "bass", "acoustic", "custom"];
/// What the T key steps through (the Python app's TONES).
const CYCLE: [&str; 5] = ["as is", "clean", "crunch", "lead", "custom"];

pub fn label(name: &str) -> &'static str {
    match name {
        "clean" => "Clean",
        "crunch" => "Crunch",
        "lead" => "Lead",
        "bass" => "Bass",
        "acoustic" => "Acoustic",
        "custom" => "Custom",
        _ => "As is",
    }
}

pub fn name_of(spec: &ToneSpec) -> &'static str {
    match spec {
        ToneSpec::AsIs => "as is",
        ToneSpec::Preset { preset } => match preset {
            Preset::Clean => "clean",
            Preset::Crunch => "crunch",
            Preset::Lead => "lead",
            Preset::Bass => "bass",
            Preset::Acoustic => "acoustic",
        },
        ToneSpec::Custom { .. } => "custom",
    }
}

pub fn spec_of(name: &str, custom: ToneParams) -> ToneSpec {
    let preset = |preset| ToneSpec::Preset { preset };
    match name {
        "clean" => preset(Preset::Clean),
        "crunch" => preset(Preset::Crunch),
        "lead" => preset(Preset::Lead),
        "bass" => preset(Preset::Bass),
        "acoustic" => preset(Preset::Acoustic),
        "custom" => ToneSpec::Custom { params: custom },
        _ => ToneSpec::AsIs,
    }
}

/// The tone after this one on the T key.
pub fn next(name: &str) -> &'static str {
    match CYCLE.iter().position(|t| *t == name) {
        Some(i) => CYCLE[(i + 1) % CYCLE.len()],
        None => CYCLE[0],
    }
}

/// The custom tone's knobs (defaults where config.json has none).
pub fn custom(cfg: &Config) -> ToneParams {
    cfg.get("tone_custom")
        .map(ToneParams::from_json)
        .unwrap_or_default()
}

/// The knobs as the Python app stores them (rounded to 3 places, its knob step).
pub fn params_json(p: &ToneParams) -> Value {
    let mut m = Map::new();
    for (k, ..) in KNOBS {
        let v = p.get(k).unwrap_or(0.0) as f64;
        m.insert(k.into(), Value::from((v * 1000.0).round() / 1000.0));
    }
    Value::Object(m)
}

/// The tone named in config.json.
pub fn current(cfg: &Config) -> ToneSpec {
    spec_of(&cfg.str("tone_v2", "as is"), custom(cfg))
}

pub fn store(cfg: &mut Config, spec: &ToneSpec) {
    cfg.set("tone_v2", name_of(spec));
    if let ToneSpec::Custom { params } = spec {
        cfg.set("tone_custom", params_json(params));
    }
}

/// The saved tones, by name (sorted).
pub fn presets(cfg: &Config) -> Vec<(String, ToneParams)> {
    let mut out: Vec<(String, ToneParams)> = cfg
        .get("tone_presets")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), ToneParams::from_json(v)))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn save_preset(cfg: &mut Config, name: &str, p: &ToneParams) {
    let mut m = cfg
        .get("tone_presets")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    m.insert(name.into(), params_json(p));
    cfg.set("tone_presets", Value::Object(m));
    cfg.set("tone_preset", name);
}

pub fn delete_preset(cfg: &mut Config, name: &str) {
    if let Some(mut m) = cfg.get("tone_presets").and_then(Value::as_object).cloned() {
        m.remove(name);
        cfg.set("tone_presets", Value::Object(m));
    }
    if cfg.str("tone_preset", "") == name {
        cfg.remove("tone_preset");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(json: &str) -> Config {
        let dir = std::env::temp_dir().join(format!("qrs-tones-{}-{}", std::process::id(), json.len()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        std::fs::write(&path, json).unwrap();
        Config::load(path)
    }

    #[test]
    fn reads_the_python_keys() {
        let c = cfg(r#"{"tone_v2": "custom", "tone_custom": {"drive": 20.0, "gate": -60}}"#);
        match current(&c) {
            ToneSpec::Custom { params } => {
                assert_eq!(params.drive, 20.0);
                assert_eq!(params.gate, -60.0);
                assert_eq!(params.room, ToneParams::default().room);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(current(&cfg("{}")), ToneSpec::AsIs);
        assert_eq!(name_of(&current(&cfg(r#"{"tone_v2": "lead"}"#))), "lead");
    }

    #[test]
    fn stores_and_cycles() {
        let mut c = cfg("{}");
        let mut p = ToneParams::default();
        p.set("delay", 0.25);
        store(&mut c, &ToneSpec::Custom { params: p });
        assert_eq!(c.str("tone_v2", ""), "custom");
        assert_eq!(custom(&c).delay, 0.25);
        save_preset(&mut c, "Warm", &p);
        assert_eq!(presets(&c)[0].0, "Warm");
        delete_preset(&mut c, "Warm");
        assert!(presets(&c).is_empty() && c.get("tone_preset").is_none());
        assert_eq!(next("lead"), "custom");
        assert_eq!(next("custom"), "as is");
        assert_eq!(next("bass"), "as is");
        for t in TONES {
            assert_eq!(name_of(&spec_of(t, p)), t);
        }
    }
}
