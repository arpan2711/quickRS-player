//! The first-time setup (P23.14, owner request; new code, no Python source): one flow over the app the first time
//! it runs, replacing the getting-started welcome (P20.2) and the guitar setup's fading first-run card (P23.8).
//! Its pages, in order: the look (the theme), low latency (Windows only: WASAPI exclusive; before the guitar since
//! P36.6, so its timing is measured on the output it will play on), your guitar (the guided guitar setup of
//! `settings/guide.rs`, run inside this flow), Use NAM? (NAM builds only, P36.7), the note widget, the folders
//! (keys, songs, recordings) and a summary. This module is what it decides: the pages and how Back / Next / Skip move through
//! them, when it opens by itself, where its choices are saved, and its pictures; `settings/setup.rs` draws it.
//!
//! config.json `first_setup`: "done" (Finish) or "skipped" (closed before the end). Either stops it opening by
//! itself; Settings' "Run the first-time setup again" always opens it. A config that already has song folders, an
//! input calibrated, the old welcome or guitar setup recorded (or keys found) is an upgrade: not forced on them.

use std::cell::RefCell;
use std::collections::HashMap;
#[cfg(test)]
use std::path::{Path, PathBuf};

use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureId, TextureOptions};
use qrs_library::Config;
use qrs_ui::theme::ThemeName;
use serde_json::Value;

use crate::guitar_setup::Mark;

/// The config.json key recording how the setup went.
pub const KEY: &str = "first_setup";

/// The pages, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Look,
    Latency,
    Guitar,
    /// Use NAM? (P36.7, D96): captures of real gear through your own TONE3000 account, or our models.
    Nam,
    Widget,
    Folders,
    Done,
}

impl Page {
    pub const ALL: [Page; 7] = [
        Page::Look,
        Page::Latency,
        Page::Guitar,
        Page::Nam,
        Page::Widget,
        Page::Folders,
        Page::Done,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Page::Look => "look",
            Page::Guitar => "guitar",
            Page::Latency => "latency",
            Page::Nam => "nam",
            Page::Widget => "widget",
            Page::Folders => "folders",
            Page::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.key() == s)
    }

    /// Its name in the step indicator.
    pub fn short(self) -> &'static str {
        match self {
            Page::Look => "Look",
            Page::Guitar => "Instrument",
            Page::Latency => "Low latency",
            Page::Nam => "NAM",
            Page::Widget => "Note widget",
            Page::Folders => "Folders",
            Page::Done => "Done",
        }
    }

    /// Its heading.
    pub fn title(self) -> &'static str {
        match self {
            Page::Look => "Pick a look",
            Page::Guitar => "Your instrument",
            Page::Latency => "Low-latency output",
            Page::Nam => "Real-gear captures?",
            Page::Widget => "The note widget",
            Page::Folders => "Keys and folders",
            Page::Done => "You're set",
        }
    }

    /// Its name in the step indicator for what you play (P37.6a): "Your bass" once Bass is picked, else
    /// "Instrument" (a guitar is the default, not a choice made yet; "Your instrument" didn't fit the step).
    pub fn short_for(self, inst: qrs_analysis::instrument::Instrument) -> &'static str {
        match self {
            Page::Guitar if inst.is_bass() => "Your bass",
            _ => self.short(),
        }
    }

    /// Its heading for what you play (as `short_for`).
    pub fn title_for(self, inst: qrs_analysis::instrument::Instrument) -> &'static str {
        match self {
            Page::Guitar if inst.is_bass() => "Your bass",
            _ => self.title(),
        }
    }

    /// Where Settings has it (the page's "change it later" line); None for the summary.
    pub fn settings_section(self) -> Option<&'static str> {
        match self {
            Page::Look => Some("Display"),
            Page::Guitar => Some("Sound"),
            Page::Latency => Some("Timing"),
            Page::Nam => Some("Neural Amp Modeler"),
            Page::Widget => Some("Display"),
            Page::Folders => Some("Library"),
            Page::Done => None,
        }
    }

    /// The page's "change it later" line.
    pub fn later(self) -> String {
        match self.settings_section() {
            Some(s) => format!("You can change this any time in Settings > {s}."),
            None => "You can run this again from Settings > About.".into(),
        }
    }
}

/// The pages this computer shows: low latency is WASAPI exclusive mode, Windows only (a Mac's Core Audio is
/// already direct, so there's nothing to choose there); Use NAM? only in a build with NAM.
pub fn pages() -> &'static [Page] {
    pages_for(cfg!(windows), crate::captures::ON)
}

pub fn pages_for(windows: bool, nam: bool) -> &'static [Page] {
    use Page::*;
    const NO_NAM: [Page; 6] = [Look, Latency, Guitar, Widget, Folders, Done];
    const MAC: [Page; 6] = [Look, Guitar, Nam, Widget, Folders, Done];
    const MAC_NO_NAM: [Page; 5] = [Look, Guitar, Widget, Folders, Done];
    match (windows, nam) {
        (true, true) => &Page::ALL,
        (true, false) => &NO_NAM,
        (false, true) => &MAC,
        (false, false) => &MAC_NO_NAM,
    }
}

/// Where the setup is, and how each page was left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
    pages: &'static [Page],
    pub page: Page,
    marks: HashMap<Page, Mark>,
}

impl Default for Flow {
    fn default() -> Self {
        Flow::new(pages())
    }
}

impl Flow {
    pub fn new(pages: &'static [Page]) -> Flow {
        Flow {
            pages,
            page: pages[0],
            marks: HashMap::new(),
        }
    }

    pub fn pages(&self) -> &'static [Page] {
        self.pages
    }

    /// Where the page is among them (0-based), and how many there are.
    pub fn place(&self) -> (usize, usize) {
        (
            self.pages.iter().position(|p| *p == self.page).unwrap_or(0),
            self.pages.len(),
        )
    }

    pub fn mark(&self, p: Page) -> Mark {
        self.marks.get(&p).copied().unwrap_or_default()
    }

    pub fn is_last(&self) -> bool {
        self.page == *self.pages.last().unwrap_or(&Page::Done)
    }

    /// Next: this page done, on to the next (it stays on the last).
    pub fn next(&mut self) {
        self.leave(Mark::Done);
    }

    /// Skip: on to the next page, this one marked skipped (unless it was done before).
    pub fn skip(&mut self) {
        self.leave(Mark::Skipped);
    }

    fn leave(&mut self, how: Mark) {
        let (i, n) = self.place();
        if i + 1 >= n {
            return;
        }
        if !(how == Mark::Skipped && self.mark(self.page) == Mark::Done) {
            self.marks.insert(self.page, how);
        }
        self.page = self.pages[i + 1];
    }

    /// Back a page (none before the first).
    pub fn back(&mut self) -> bool {
        let (i, _) = self.place();
        if i == 0 {
            return false;
        }
        self.page = self.pages[i - 1];
        true
    }

    /// Straight to a page (the step indicator's), if this computer shows it.
    pub fn go(&mut self, p: Page) -> bool {
        if self.pages.contains(&p) {
            self.page = p;
            true
        } else {
            false
        }
    }
}

// -- when it opens -----------------------------------------------------------------------------------------------

/// Whether any input has been calibrated.
pub fn any_calibrated(cfg: &Config) -> bool {
    cfg.get("input_cal")
        .and_then(Value::as_object)
        .is_some_and(|m| m.values().any(|v| v.get("peak_db").is_some()))
}

/// An existing user's config (an upgrade from an earlier quickRS, or the Python app's, brought over): song folders
/// added, an input calibrated, the old welcome or guitar setup gone through, or keys found.
pub fn upgrader(cfg: &Config, keys_found: bool) -> bool {
    keys_found
        || !cfg.added_dirs().is_empty()
        || any_calibrated(cfg)
        || cfg.bool("welcome_done", false)
        || cfg.get(crate::guitar_setup::KEY).is_some()
}

/// Whether the setup opens by itself: never once it was done or skipped; on a first run (no config.json at the
/// start); otherwise only for a config with nothing of an existing user's in it.
pub fn due(cfg: &Config, first_run: bool, keys_found: bool) -> bool {
    if cfg.get(KEY).is_some() {
        return false;
    }
    first_run || !upgrader(cfg, keys_found)
}

/// Record how it went ("done" or "skipped"); "done" isn't undone by a later skip (a run from Settings closed early).
pub fn record(cfg: &mut Config, what: &str) {
    if cfg.get(KEY).and_then(Value::as_str) == Some("done") && what != "done" {
        return;
    }
    cfg.set(KEY, what);
}

// -- where its choices go (where Settings reads them) ------------------------------------------------------------

/// The theme, as the sidebar's pick and Settings > Display save it.
pub fn set_theme(cfg: &mut Config, name: ThemeName) {
    cfg.set("theme", name.key());
    if name != ThemeName::Daylight {
        cfg.set("dark_theme", name.key());
    }
}

/// Low-latency output (WASAPI exclusive) in config.json's `audio`, as Settings > Audio saves it.
pub fn set_low_latency(cfg: &mut Config, on: bool) {
    let mut prefs: qrs_audio::AudioPrefs = cfg
        .get("audio")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    prefs.exclusive = on;
    cfg.set("audio", serde_json::to_value(&prefs).unwrap_or_default());
}

pub fn low_latency(cfg: &Config) -> bool {
    cfg.get("audio")
        .and_then(|v| serde_json::from_value::<qrs_audio::AudioPrefs>(v.clone()).ok())
        .is_some_and(|p| p.exclusive)
}

/// The note widget on or off (Settings > Audio's switch; on unless switched off).
pub fn set_note_widget(cfg: &mut Config, on: bool) {
    cfg.set("note_widget", on);
}

pub fn note_widget(cfg: &Config) -> bool {
    cfg.bool("note_widget", true)
}

pub use qrs_library::config::default_songs_dir;

/// Use NAM? (P36.7): config.json `nam_setup` ("yes" / "no"), and `nam_songs` to match (songs open on captures, or
/// on our models; D82's switch).
pub fn set_nam(cfg: &mut Config, on: bool) {
    cfg.set("nam_setup", if on { "yes" } else { "no" });
    cfg.set("nam_songs", on);
    crate::captures::set_songs_use_nam(on);
}

/// What was answered (None: not asked yet).
pub fn nam_choice(cfg: &Config) -> Option<bool> {
    match cfg.get("nam_setup").and_then(Value::as_str) {
        Some("yes") => Some(true),
        Some("no") => Some(false),
        _ => None,
    }
}

// -- the pictures ------------------------------------------------------------------------------------------------

/// The pictures beside the pages: crops of the app's own screenshots (`tools/setup-shots.py` makes them from the
/// shared build with scratch data), compiled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pic {
    Theme(ThemeName),
    Guitar,
    Latency,
    Widget,
    Folders,
    Room,
}

impl Pic {
    fn bytes(self) -> &'static [u8] {
        match self {
            Pic::Theme(ThemeName::Midnight) => include_bytes!("../../../assets/setup/theme-midnight.png"),
            Pic::Theme(ThemeName::Graphite) => include_bytes!("../../../assets/setup/theme-graphite.png"),
            Pic::Theme(ThemeName::Daylight) => include_bytes!("../../../assets/setup/theme-daylight.png"),
            Pic::Guitar => include_bytes!("../../../assets/setup/guitar.png"),
            Pic::Latency => include_bytes!("../../../assets/setup/latency.png"),
            Pic::Widget => include_bytes!("../../../assets/setup/widget.png"),
            Pic::Folders => include_bytes!("../../../assets/setup/folders.png"),
            Pic::Room => include_bytes!("../../../assets/setup/room.png"),
        }
    }

    /// The picture for a page (the look page has its three themes instead).
    pub fn of(page: Page) -> Option<Pic> {
        match page {
            Page::Look => None,
            Page::Guitar => Some(Pic::Guitar),
            Page::Latency => Some(Pic::Latency),
            Page::Nam => None,
            Page::Widget => Some(Pic::Widget),
            Page::Folders => Some(Pic::Folders),
            Page::Done => Some(Pic::Room),
        }
    }
}

thread_local! {
    static PICS: RefCell<HashMap<String, Option<(TextureHandle, [usize; 2])>>> = RefCell::new(HashMap::new());
}

/// A picture's texture and its size in pixels, loaded once.
pub fn picture(ctx: &egui::Context, pic: Pic) -> Option<(TextureId, [usize; 2])> {
    PICS.with(|m| {
        let mut m = m.borrow_mut();
        m.entry(format!("{pic:?}"))
            .or_insert_with(|| {
                let img = image::load_from_memory(pic.bytes()).ok()?.to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                let tex = ctx.load_texture(
                    format!("setup-{pic:?}"),
                    ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                    TextureOptions::LINEAR,
                );
                Some((tex, size))
            })
            .as_ref()
            .map(|(t, s)| (t.id(), *s))
    })
}

#[cfg(test)]
mod tests {
    /// P37.6a: the step is "Instrument" ("Your instrument" over it) until Bass is picked, then "Your bass".
    #[test]
    fn the_instrument_page_is_named_for_what_you_play() {
        use qrs_analysis::instrument::Instrument;
        assert_eq!(super::Page::Guitar.short_for(Instrument::Guitar), "Instrument");
        assert_eq!(
            super::Page::Guitar.title_for(Instrument::Guitar),
            "Your instrument"
        );
        assert_eq!(super::Page::Guitar.short_for(Instrument::Bass5), "Your bass");
        assert_eq!(super::Page::Guitar.title_for(Instrument::Bass), "Your bass");
        assert_eq!(super::Page::Look.short_for(Instrument::Bass), "Look");
    }

    use super::*;
    use qrs_library::config::songs_in;

    #[test]
    fn it_opens_on_a_fresh_config_and_not_for_an_upgrade() {
        // no config.json: straight away
        let fresh = Config::default();
        assert!(due(&fresh, true, false));
        // a config with nothing of a user's in it (a start closed straight away): still due
        let mut bare = Config::default();
        bare.set("theme", "midnight");
        assert!(due(&bare, false, false));
        // upgrades: song folders, keys, a calibrated input, the old welcome or guitar setup
        let mut c = Config::default();
        c.set_library_dirs(&[PathBuf::from("D:/Songs")]);
        assert!(!due(&c, false, false));
        assert!(!due(&bare, false, true)); // keys found
        let mut c = Config::default();
        c.set(
            "input_cal",
            serde_json::json!({"Asio:X": {"trim_db": 3.0, "peak_db": -15.0}}),
        );
        assert!(!due(&c, false, false));
        for (k, v) in [
            ("welcome_done", Value::from(true)),
            ("guitar_setup", "dismissed".into()),
        ] {
            let mut c = Config::default();
            c.set(k, v);
            assert!(!due(&c, false, false), "{k}");
        }
        // done or skipped: never again, not even on a "first run"
        for what in ["done", "skipped"] {
            let mut c = Config::default();
            record(&mut c, what);
            assert!(!due(&c, true, false), "{what}");
            assert!(!due(&c, false, false), "{what}");
        }
        // done stays done
        let mut c = Config::default();
        record(&mut c, "done");
        record(&mut c, "skipped");
        assert_eq!(c.get(KEY).and_then(Value::as_str), Some("done"));
    }

    #[test]
    fn the_pages_go_in_order_back_and_skip() {
        let mut f = Flow::new(pages_for(true, false));
        assert_eq!(
            f.pages(),
            [
                Page::Look,
                Page::Latency,
                Page::Guitar,
                Page::Widget,
                Page::Folders,
                Page::Done
            ]
        );
        assert_eq!(f.page, Page::Look);
        assert!(!f.back()); // nothing before the first
        f.next();
        assert_eq!((f.page, f.mark(Page::Look)), (Page::Latency, Mark::Done));
        f.skip();
        assert_eq!((f.page, f.mark(Page::Latency)), (Page::Guitar, Mark::Skipped));
        assert!(f.back());
        assert_eq!(f.page, Page::Latency);
        f.next(); // done after all
        assert_eq!(f.mark(Page::Latency), Mark::Done);
        assert!(f.back());
        f.skip(); // a skip doesn't undo a page that was done
        assert_eq!(f.mark(Page::Latency), Mark::Done);
        assert_eq!(f.place(), (2, 6));
        // low latency comes before the guitar (its timing is measured on that output), NAM after it
        let all = pages_for(true, true);
        let at = |p: Page| all.iter().position(|q| *q == p).unwrap();
        assert!(at(Page::Latency) < at(Page::Guitar) && at(Page::Guitar) < at(Page::Nam));
        assert!(!pages_for(false, false).contains(&Page::Nam));
        for _ in 0..10 {
            f.next();
        }
        assert!(f.is_last());
        assert_eq!(f.page, Page::Done); // it stays on the last
        assert!(f.go(Page::Widget));
        assert_eq!(f.page, Page::Widget);
        for p in Page::ALL {
            assert_eq!(Page::parse(p.key()), Some(p));
        }
        // a Mac has no low-latency page
        let mut m = Flow::new(pages_for(false, false));
        assert!(!m.pages().contains(&Page::Latency));
        m.next();
        m.next();
        assert_eq!(m.page, Page::Widget);
        let mut c = Config::default();
        assert_eq!(nam_choice(&c), None);
        set_nam(&mut c, false);
        assert_eq!(nam_choice(&c), Some(false));
        assert_eq!(c.get("nam_songs").and_then(Value::as_bool), Some(false));
        set_nam(&mut c, true);
        assert_eq!(nam_choice(&c), Some(true));
        assert!(!m.go(Page::Latency));
    }

    #[test]
    fn every_page_says_where_to_change_it() {
        for p in Page::ALL {
            let l = p.later();
            assert!(l.contains("Settings"), "{p:?}");
            if p != Page::Done {
                assert!(l.starts_with("You can change this any time in Settings > "));
            }
        }
        assert_eq!(Page::Folders.settings_section(), Some("Library"));
        assert_eq!(Page::Latency.settings_section(), Some("Timing"));
    }

    #[test]
    fn choices_are_saved_where_settings_reads_them() {
        let mut c = Config::default();
        set_theme(&mut c, ThemeName::Graphite);
        assert_eq!(c.str("theme", ""), "graphite");
        assert_eq!(c.str("dark_theme", ""), "graphite");
        set_theme(&mut c, ThemeName::Daylight);
        assert_eq!(c.str("theme", ""), "daylight");
        assert_eq!(c.str("dark_theme", ""), "graphite"); // (the sidebar's dark one kept)
        // low latency: in the audio prefs the engine starts from, the rest of them kept
        assert!(!low_latency(&c));
        c.set("audio", serde_json::json!({"buffer": 128}));
        set_low_latency(&mut c, true);
        let p: qrs_audio::AudioPrefs = serde_json::from_value(c.get("audio").cloned().unwrap()).unwrap();
        assert!(p.exclusive);
        assert_eq!(p.buffer, Some(128));
        assert!(low_latency(&c));
        set_low_latency(&mut c, false);
        assert!(!low_latency(&c));
        // the note widget: on unless switched off
        assert!(note_widget(&c));
        set_note_widget(&mut c, false);
        assert!(!c.bool("note_widget", true));
        // folders: a songs folder added is in library_dirs (Settings > Library's list); recordings' folders
        let songs = songs_in(Some(Path::new("M:/Music")), Path::new("D:/data"));
        assert_eq!(songs, Path::new("M:/Music").join("quickRS Songs"));
        assert_eq!(
            songs_in(None, Path::new("D:/data")),
            Path::new("D:/data").join("songs")
        );
        c.add_dir(&songs);
        assert!(c.added_dirs().iter().any(|d| d == &songs));
        let rec = std::env::temp_dir().join("Rec"); // (absolute on every OS; "M:/Rec" isn't on a Mac)
        c.set_folder(qrs_library::config::RECORDINGS_DIR, Some(&rec));
        assert_eq!(c.recordings_dir(Path::new("D:/data")), rec);
        // and an upgrade now: no setup forced on the next start
        assert!(upgrader(&c, false));
    }

    #[test]
    fn keys_saved_on_the_folders_page_are_read_as_settings_reads_them() {
        let dir = std::env::temp_dir().join(format!("qrs-setup-keys-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("keys.json");
        let k = |b: u8| {
            crate::keyfile::check_key(&format!("{b:02x}").repeat(32))
                .unwrap()
                .unwrap()
        };
        crate::keyfile::write(&path, k(0xab), k(0xcd)).unwrap();
        let (_, state) = crate::keyfile::load(std::slice::from_ref(&path));
        assert!(matches!(state, crate::keyfile::KeyState::Found { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_picture_is_compiled_in_and_reads() {
        let mut pics = vec![Pic::Guitar, Pic::Latency, Pic::Widget, Pic::Folders, Pic::Room];
        pics.extend(ThemeName::ALL.map(Pic::Theme));
        let mut total = 0;
        for p in pics {
            let img = image::load_from_memory(p.bytes()).unwrap();
            assert!(img.width() > 100 && img.height() > 60, "{p:?}");
            total += p.bytes().len();
        }
        assert!(total < 600_000, "the pictures are {total} bytes");
        for p in Page::ALL {
            assert_eq!(Pic::of(p).is_none(), matches!(p, Page::Look | Page::Nam));
        }
    }
}
