//! "Reset <page> to defaults" and "Reset all settings…" (P35.7, D92; new in quickRS): the config.json keys each page
//! owns go back to their defaults (removed, so the app's own default reads), and the device prefs the page shows
//! (config.json `audio`) are set back field by field (kept as an object: without one, the Python app's old device
//! keys would be read again on the next start). No key is renamed; the Python app reads the same file.
//!
//! Every reset keeps your song folders, RS's keys, scores, saved licks and lessons, each input's calibration
//! and the theme you picked; Reset all also keeps the tone (the choice, the custom knobs and the saved tones).

use qrs_audio::AudioPrefs;
use qrs_audio::tone::ToneParams;
use qrs_library::Config;
use serde_json::{Value, json};

use crate::player::tiles::TILES;
use crate::update;

/// The config.json keys a page owns (what its reset removes).
pub fn keys(page: &str) -> &'static [&'static str] {
    match page {
        "sound" | "audio" => &["volume", "guitar_volume", "monitor", "monitor_mic"],
        // display_offset: the Screen timing, once the timing tool brings it (removing a missing key does nothing)
        "timing" | "practice" => &["input_offset", "av_offset", "display_offset", "countdown"],
        "tone" => &["tone_v2", "tone_custom", "tone_preset"],
        "display" => &[
            "knobs",
            "string_colors",
            "note_view",
            "notation",
            "tiles",
            "lyrics_next",
            "note_widget",
            "tuner_face",
        ],
        "library" => &["select_preview", "prefer", "recordings_dir", "takes_dir"],
        "about" | "updates" => &[update::KEY_CHECK, update::KEY_SKIP],
        // (the captures, the TONE3000 key and NAM for songs are kept: only the reference level, P37.15)
        "nam" => &["nam_input_dbu"],
        _ => &[],
    }
}

/// The pages Reset all resets (all but Tone: your tone is kept).
pub const ALL: [&str; 6] = ["sound", "timing", "display", "library", "about", "nam"];

/// Never touched by a reset: folders, keys, scores, tones, calibration, the theme, the first-time setup's record.
/// (Listed for the tests: no page owns one of them.)
#[cfg(test)]
pub const KEPT: [&str; 15] = [
    "nam_setup",
    "nam_songs",
    "nam_page",
    "tone3000_client_id",
    "library_dirs",
    "library_dirs_removed",
    qrs_library::config::SEARCH_RS_DLC,
    "tone_presets",
    "tone_by_input",
    "input_cal",
    "theme",
    "dark_theme",
    "guitar_setup",
    "first_setup",
    "update_last_check",
];

/// The device prefs a page shows, back to their defaults (Sound: the devices, the channel, the RS setup;
/// Timing: low-latency output and the buffer).
pub fn reset_prefs(prefs: &mut AudioPrefs, page: &str) {
    let d = AudioPrefs::default();
    match page {
        "sound" | "audio" => {
            prefs.input = d.input;
            prefs.output = d.output;
            prefs.channel = d.channel;
            prefs.use_rs_setup = d.use_rs_setup;
        }
        "timing" | "practice" => {
            prefs.exclusive = d.exclusive;
            prefs.buffer = d.buffer;
        }
        _ => {}
    }
}

/// The device prefs as config.json has them.
pub fn prefs_of(cfg: &Config) -> AudioPrefs {
    cfg.get("audio")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Reset a page in `cfg` (the caller applies it to the engine and saves).
pub fn reset(cfg: &mut Config, page: &str) {
    for k in keys(page) {
        cfg.remove(k);
    }
    if matches!(page, "sound" | "audio" | "timing" | "practice") {
        let mut prefs = prefs_of(cfg);
        reset_prefs(&mut prefs, page);
        cfg.set("audio", serde_json::to_value(&prefs).unwrap_or_default());
    }
}

/// Reset every page but Tone.
pub fn reset_all(cfg: &mut Config) {
    for page in ALL {
        reset(cfg, page);
    }
}

/// A plain key's value when it isn't set (None: any value set is a change, as a folder is).
fn default_of(key: &str) -> Option<Value> {
    Some(match key {
        "volume" | "guitar_volume" => json!(0.8),
        "monitor" | "lyrics_next" | "note_widget" | "select_preview" | "countdown" | update::KEY_CHECK => {
            json!(true)
        }
        "monitor_mic" | "string_colors" => json!(false),
        "input_offset" | "av_offset" | "display_offset" => json!(0.0),
        "nam_input_dbu" => json!(qrs_audio::board::nam::INPUT_DBU),
        "tone_v2" => json!("as is"),
        "knobs" => json!("amp"),
        "note_view" => json!("piano"),
        update::KEY_SKIP => json!(""),
        _ => return None,
    })
}

/// Whether a key's saved value is what it'd be unset (numbers to a millionth; the notation's switches all on;
/// the tiles in their first order, each shown but the MG-300's; the custom knobs where they start).
pub fn is_default(key: &str, v: &Value) -> bool {
    match key {
        // which saved tone is picked isn't a change by itself
        "tone_preset" => true,
        "notation" => v
            .as_object()
            .is_none_or(|m| m.values().all(|x| x.as_bool() != Some(false))),
        "tiles" => {
            let Some(items) = v.as_array() else {
                return true;
            };
            let saved: Vec<(&str, bool)> = items
                .iter()
                .filter_map(|i| Some((i.get(0)?.as_str()?, i.get(1)?.as_bool()?)))
                .collect();
            let first: Vec<&str> = TILES
                .iter()
                .map(|t| t.0)
                .filter(|k| saved.iter().any(|s| s.0 == *k))
                .collect();
            saved.iter().map(|s| s.0).eq(first) && saved.iter().all(|(k, on)| *on == (*k != "mg300"))
        }
        "tone_custom" => ToneParams::from_json(v) == ToneParams::default(),
        _ => match (default_of(key), v.as_f64()) {
            (Some(d), Some(x)) if d.is_number() => (d.as_f64().unwrap_or(0.0) - x).abs() < 1e-6,
            (Some(d), _) => d == *v,
            (None, _) => false,
        },
    }
}

/// Whether anything a page owns is off its default (the reset link is greyed out otherwise).
pub fn differs(cfg: &Config, page: &str) -> bool {
    let prefs = prefs_of(cfg);
    let mut reset_ = prefs.clone();
    reset_prefs(&mut reset_, page);
    let keys_differ = keys(page)
        .iter()
        .any(|k| cfg.get(k).is_some_and(|v| !is_default(k, v)));
    keys_differ || reset_ != prefs
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_audio::{Choice, DevicePref};
    use serde_json::json;

    fn full() -> Config {
        let mut c = Config::default();
        for page in ["sound", "timing", "tone", "display", "library", "about"] {
            for k in keys(page) {
                c.set(k, json!(1));
            }
        }
        for k in KEPT {
            c.set(k, json!("kept"));
        }
        c.set("scores_x", json!(3));
        let prefs = AudioPrefs {
            input: Choice::Device(nux()),
            channel: Some(1),
            exclusive: true,
            buffer: Some(128),
            use_rs_setup: false,
            ..Default::default()
        };
        c.set("audio", serde_json::to_value(&prefs).unwrap());
        c
    }

    fn nux() -> DevicePref {
        DevicePref {
            backend: qrs_audio::backend::Backend::Asio,
            id: "NUX Audio".into(),
            name: "NUX Audio".into(),
        }
    }

    #[test]
    fn no_page_owns_a_kept_key_and_pages_dont_share() {
        let pages = ["sound", "timing", "tone", "display", "library", "about"];
        for page in pages {
            for k in keys(page) {
                assert!(!KEPT.contains(k), "{page} resets {k}");
                for other in pages.iter().filter(|o| **o != page) {
                    assert!(!keys(other).contains(k), "{k} on {page} and {other}");
                }
            }
        }
        // the old ids reset the same as the new
        assert_eq!(keys("audio"), keys("sound"));
        assert_eq!(keys("practice"), keys("timing"));
        assert_eq!(keys("updates"), keys("about"));
        assert!(!ALL.contains(&"tone"));
    }

    #[test]
    fn a_page_reset_touches_only_its_own() {
        let mut c = full();
        let before = prefs_of(&c);
        assert!(before.exclusive && before.buffer == Some(128));
        assert!(differs(&c, "timing"));
        reset(&mut c, "timing");
        assert!(!differs(&c, "timing"));
        assert!(c.get("input_offset").is_none() && c.get("av_offset").is_none());
        let p = prefs_of(&c);
        assert!(!p.exclusive && p.buffer.is_none());
        // the Sound page's device choice and the other pages' keys stay
        assert_eq!(p.input, before.input);
        assert_eq!(p.channel, Some(1));
        assert!(c.get("volume").is_some() && c.get("knobs").is_some() && c.get("tone_v2").is_some());
        reset(&mut c, "sound");
        let p = prefs_of(&c);
        assert_eq!(p.input, Choice::Auto);
        assert_eq!(p.channel, None);
        assert!(p.use_rs_setup);
        assert!(c.get("volume").is_none() && c.get("monitor").is_none());
        // the prefs stay an object (or the Python app's old device keys would be read again)
        assert!(c.get("audio").is_some_and(|v| v.is_object()));
    }

    #[test]
    fn a_value_saved_at_its_default_is_no_change() {
        let mut c = Config::default();
        c.set("volume", json!(0.8));
        c.set("monitor", json!(true));
        assert!(!differs(&c, "sound"));
        c.set("volume", json!(0.5));
        assert!(differs(&c, "sound"));
        c.set("tone_v2", json!("as is"));
        c.set("tone_preset", json!("Mine"));
        c.set("tone_custom", tones_default());
        assert!(!differs(&c, "tone"));
        c.set("tone_v2", json!("custom"));
        assert!(differs(&c, "tone"));
        c.set("notation", json!({"show": true, "rests": true}));
        c.set("tiles", json!([["board", true], ["note", true], ["tuner", true]]));
        assert!(!differs(&c, "display"));
        c.set("tiles", json!([["note", true], ["board", true]]));
        assert!(differs(&c, "display"));
        c.set("tiles", json!([["board", false]]));
        assert!(differs(&c, "display"));
        c.set("tiles", json!([["board", true]]));
        c.set("notation", json!({"show": false}));
        assert!(differs(&c, "display"));
        c.set("input_offset", json!(0.0));
        assert!(!differs(&c, "timing"));
        c.set("input_offset", json!(0.03));
        assert!(differs(&c, "timing"));
    }

    fn tones_default() -> Value {
        crate::tones::params_json(&ToneParams::default())
    }

    #[test]
    fn reset_all_keeps_folders_keys_scores_and_tones() {
        let mut c = full();
        reset_all(&mut c);
        for k in KEPT {
            assert_eq!(c.str(k, ""), "kept", "{k}");
        }
        assert!(c.get("scores_x").is_some());
        assert!(c.get("tone_v2").is_some() && c.get("tone_custom").is_some());
        for page in ALL {
            assert!(!differs(&c, page), "{page}");
        }
        assert_eq!(prefs_of(&c), AudioPrefs::default());
        // a device chosen by hand is a difference
        let p = AudioPrefs {
            output: Choice::Device(nux()),
            ..Default::default()
        };
        c.set("audio", serde_json::to_value(&p).unwrap());
        assert!(differs(&c, "sound"));
        assert!(!differs(&c, "timing"));
    }
}
