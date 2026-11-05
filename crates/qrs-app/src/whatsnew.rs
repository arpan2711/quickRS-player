//! After an update: "run the setup again?" on Home (P37.15, D148; new in quickRS, owner request). quickRS keeps the
//! version it last ran (config.json `last_run_version`). A start on a newer version than that, with a config that was
//! there before (an update through About's Update now, or a new zip over an old folder), shows a calm card on Home
//! on that version's first two starts: what's new, and Run setup / Not now / Don't ask again. A fresh install gets
//! the first-time setup itself instead; 0.4.0 and older never wrote the key, so a config without it is taken as
//! 0.4.0's.
//!
//! Keys (new; the Python app ignores them): `last_run_version` ("0.5.0"), `setup_prompt_for` (the version the card
//! is for), `setup_prompt_shown` (the starts it showed on), `setup_prompt_off` (Don't ask again: never again).

use qrs_library::Config;
use serde_json::Value;

use crate::update::Version;

pub const KEY_LAST: &str = "last_run_version";
pub const KEY_FOR: &str = "setup_prompt_for";
pub const KEY_SHOWN: &str = "setup_prompt_shown";
pub const KEY_OFF: &str = "setup_prompt_off";
/// The version a config without `last_run_version` is taken to come from (the last one that didn't write it).
pub const BEFORE_KEY: &str = "0.4.0";
/// How many starts the card shows on.
pub const STARTS: u64 = 2;

/// What's new since 0.4.0, a line each (the card lists them).
pub const WHATS_NEW: [&str; 6] = [
    "A new first-time setup, with a timing tool that measures your delay",
    "Bass mode: bass parts, bass amps and a bass tuner",
    "The amp room: every amp, pedal and cab on one screen, with a catalogue",
    "Neural Amp Modeler captures through your own TONE3000 account (Settings › Neural Amp Modeler)",
    "A difficulty for every part, and easy songs to start with on Home",
    "Practice that works together: loops, count-in, review and takes",
];

/// Run once at start (before the first-time setup is checked): records this version as the last run and says
/// whether the card shows this start (counted as shown). `first_run`: there was no config.json; `setup_due`: the
/// first-time setup opens by itself this start (it's the setup already: no card).
pub fn on_start(cfg: &mut Config, first_run: bool, setup_due: bool, now: &Version) -> bool {
    let last = match cfg.get(KEY_LAST).and_then(Value::as_str) {
        Some(v) => Version::parse(v),
        None if first_run => None,
        None => Version::parse(BEFORE_KEY),
    };
    cfg.set(KEY_LAST, now.to_string());
    if last.as_ref().is_some_and(|l| l < now) {
        // an update: the card is for this version (from its first start)
        cfg.set(KEY_FOR, now.to_string());
        cfg.set(KEY_SHOWN, if setup_due { STARTS } else { 0 });
    }
    if setup_due || cfg.bool(KEY_OFF, false) {
        return false;
    }
    let for_now = cfg
        .get(KEY_FOR)
        .and_then(Value::as_str)
        .and_then(Version::parse)
        .is_some_and(|v| v == *now);
    let shown = cfg.get(KEY_SHOWN).and_then(Value::as_u64).unwrap_or(0);
    if !for_now || shown >= STARTS {
        return false;
    }
    cfg.set(KEY_SHOWN, shown + 1);
    true
}

/// The card's answers: "setup" (Run setup: done with it), "later" (Not now: this start only; it was counted),
/// "never" (Don't ask again).
pub fn answer(cfg: &mut Config, what: &str) {
    match what {
        "setup" => cfg.set(KEY_SHOWN, STARTS),
        "never" => cfg.set(KEY_OFF, true),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(name: &str, json: &str) -> Config {
        let dir = std::env::temp_dir().join(format!("qrs-whatsnew-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("config.json");
        std::fs::write(&p, json).unwrap();
        Config::load(&p)
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn a_fresh_install_gets_no_card() {
        let mut c = cfg("fresh", "{}");
        assert!(!on_start(&mut c, true, true, &v("0.5.0")));
        assert_eq!(c.get(KEY_LAST).and_then(Value::as_str), Some("0.5.0"));
        // and the starts after it: the same version, no card
        assert!(!on_start(&mut c, false, false, &v("0.5.0")));
        assert!(!on_start(&mut c, false, false, &v("0.5.0")));
    }

    #[test]
    fn the_same_version_again_gets_no_card() {
        let mut c = cfg("same", r#"{"last_run_version": "0.5.0", "library_dirs": ["x"]}"#);
        assert!(!on_start(&mut c, false, false, &v("0.5.0")));
    }

    #[test]
    fn from_0_4_0_the_card_shows_twice() {
        // 0.4.0's config: no last_run_version
        let mut c = cfg("from040", r#"{"library_dirs": ["x"], "first_setup": "done"}"#);
        assert!(on_start(&mut c, false, false, &v("0.5.0")));
        assert_eq!(c.get(KEY_LAST).and_then(Value::as_str), Some("0.5.0"));
        answer(&mut c, "later");
        assert!(on_start(&mut c, false, false, &v("0.5.0")));
        assert!(
            !on_start(&mut c, false, false, &v("0.5.0")),
            "two starts, then it stops"
        );
        // a 0.4.0 build reading its own config: nothing new
        let mut c = cfg("on040", r#"{"library_dirs": ["x"]}"#);
        assert!(!on_start(&mut c, false, false, &v("0.4.0")));
    }

    #[test]
    fn a_newer_version_asks_again_unless_told_not_to() {
        let mut c = cfg("newer", r#"{"last_run_version": "0.5.0"}"#);
        assert!(on_start(&mut c, false, false, &v("0.5.1")));
        answer(&mut c, "setup");
        assert!(
            !on_start(&mut c, false, false, &v("0.5.1")),
            "Run setup: done with it"
        );
        assert!(
            on_start(&mut c, false, false, &v("0.6.0")),
            "the next update asks again"
        );
        answer(&mut c, "never");
        assert!(!on_start(&mut c, false, false, &v("0.6.0")));
        assert!(
            !on_start(&mut c, false, false, &v("0.7.0")),
            "Don't ask again: never again"
        );
        // an older version run after a newer one (a rollback): no card
        let mut c = cfg("older", r#"{"last_run_version": "0.6.0"}"#);
        assert!(!on_start(&mut c, false, false, &v("0.5.0")));
    }

    #[test]
    fn the_first_time_setup_opening_takes_the_cards_place() {
        let mut c = cfg("due", r#"{"last_run_version": "0.4.0"}"#);
        assert!(!on_start(&mut c, false, true, &v("0.5.0")));
        assert!(
            !on_start(&mut c, false, false, &v("0.5.0")),
            "and it isn't owed later"
        );
    }
}
