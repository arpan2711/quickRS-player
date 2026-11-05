//! Settings' pages, their ids (the old ones kept as aliases), the folds and the panel's own keys (P35.3, D92; new
//! in quickRS: the Python app's settingsview.py had five pages and no folds). Pure, so it's tested here.

use eframe::egui::{Key, Modifiers};

/// The pages, in the side list's order: (id, name, the one-line hint under it).
pub const PAGES: [(&str, &str, &str); 6] = [
    ("sound", "Sound", "instrument, devices, levels"),
    ("timing", "Timing", "sync, latency check"),
    ("tone", "Tone", "what you hear"),
    ("display", "Display", "look, tiles, notation"),
    ("library", "Library", "folders, keys, recordings"),
    ("about", "About", "updates, bugs, setup"),
];

/// A NAM build's pages: the same, and Neural Amp Modeler last (P37.15, D146: everything NAM in one place).
pub const PAGES_NAM: [(&str, &str, &str); 7] = [
    PAGES[0],
    PAGES[1],
    PAGES[2],
    PAGES[3],
    PAGES[4],
    PAGES[5],
    ("nam", "Neural Amp Modeler", "real-gear captures"),
];

/// The classic look's pages (the Python app's five, for the parity screenshots): no hints.
pub const CLASSIC: [(&str, &str, &str); 5] = [
    ("audio", "Audio input / output", ""),
    ("tone", "Raw guitar tone", ""),
    ("display", "Display", ""),
    ("practice", "Practice", ""),
    ("library", "Directories", ""),
];

pub fn pages(classic: bool) -> &'static [(&'static str, &'static str, &'static str)] {
    pages_for(classic, crate::captures::ON)
}

pub fn pages_for(classic: bool, nam: bool) -> &'static [(&'static str, &'static str, &'static str)] {
    match (classic, nam) {
        (true, _) => &CLASSIC,
        (false, true) => &PAGES_NAM,
        (false, false) => &PAGES,
    }
}

/// The page an id opens: the page itself, or the page an old id now lives on (audio is Sound, practice is Timing,
/// updates is About; in the classic look the other way round). None when it's no page.
pub fn resolve(id: &str, classic: bool) -> Option<&'static str> {
    let id = if classic {
        match id {
            "sound" => "audio",
            "timing" => "practice",
            x => x,
        }
    } else {
        match id {
            "audio" => "sound",
            "practice" => "timing",
            "updates" => "about",
            // (the NAM bits that had homes elsewhere: Tone's Suggested captures, the setup's Use NAM?)
            "captures" | "tone3000" | "nam_setup" => "nam",
            x => x,
        }
    };
    pages(classic).iter().find(|p| p.0 == id).map(|p| p.0)
}

/// The Sound page's id in this look (the guided setup and the level tool run there).
pub fn sound(classic: bool) -> &'static str {
    if classic { "audio" } else { "sound" }
}

/// The page `by` pages on from `cur` (wrapping round).
pub fn step(classic: bool, cur: &str, by: i32) -> &'static str {
    let list = pages(classic);
    let n = list.len() as i32;
    let i = list.iter().position(|p| p.0 == cur).unwrap_or(0) as i32;
    list[(i + by).rem_euclid(n) as usize].0
}

/// What one of the panel's own keys does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nav {
    Close,
    /// Scroll the page by this many points (down is positive).
    Scroll(f32),
    /// Another page, this many on (-1 the one above).
    Page(i32),
}

/// The panel's keys: Esc closes (Backspace doesn't: it's a text key), Up / Down scroll, Page Up / Down a screen,
/// Tab / Shift+Tab and Ctrl+Up / Ctrl+Down switch pages.
pub fn nav_key(key: Key, m: Modifiers) -> Option<Nav> {
    match key {
        Key::Escape => Some(Nav::Close),
        Key::Tab => Some(Nav::Page(if m.shift { -1 } else { 1 })),
        Key::ArrowUp if m.command => Some(Nav::Page(-1)),
        Key::ArrowDown if m.command => Some(Nav::Page(1)),
        Key::ArrowUp => Some(Nav::Scroll(-60.0)),
        Key::ArrowDown => Some(Nav::Scroll(60.0)),
        Key::PageUp => Some(Nav::Scroll(-400.0)),
        Key::PageDown => Some(Nav::Scroll(400.0)),
        _ => None,
    }
}

/// Which folds are open (one level deep; kept while the panel is open, closed again the next time).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Folds(Vec<String>);

impl Folds {
    pub fn is_open(&self, id: &str) -> bool {
        self.0.iter().any(|f| f == id)
    }

    pub fn toggle(&mut self, id: &str) {
        if let Some(i) = self.0.iter().position(|f| f == id) {
            self.0.remove(i);
        } else {
            self.0.push(id.to_string());
        }
    }

    pub fn open(&mut self, id: &str) {
        if !self.is_open(id) {
            self.0.push(id.to_string());
        }
    }

    pub fn close(&mut self, id: &str) {
        self.0.retain(|f| f != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_ids_open_the_new_pages() {
        assert_eq!(resolve("audio", false), Some("sound"));
        assert_eq!(resolve("practice", false), Some("timing"));
        assert_eq!(resolve("updates", false), Some("about"));
        for (id, ..) in pages(false).iter().copied() {
            assert_eq!(resolve(id, false), Some(id));
        }
        assert_eq!(resolve("nope", false), None);
        // the classic look keeps the old five: the new ids find them, About isn't there
        assert_eq!(resolve("sound", true), Some("audio"));
        assert_eq!(resolve("timing", true), Some("practice"));
        assert_eq!(resolve("library", true), Some("library"));
        assert_eq!(resolve("about", true), None);
        assert_eq!(pages(true).len(), 5);
        assert_eq!(pages_for(false, false).len(), 6);
        // a NAM build: Neural Amp Modeler, last; its old homes open it; no NAM, no page
        assert_eq!(pages_for(false, true).len(), 7);
        assert_eq!(pages_for(false, true).last().unwrap().0, "nam");
        assert_eq!(pages_for(true, true).len(), 5);
        if crate::captures::ON {
            assert_eq!(resolve("captures", false), Some("nam"));
            assert_eq!(resolve("tone3000", false), Some("nam"));
            assert_eq!(resolve("nam", true), None);
        } else {
            assert_eq!(resolve("nam", false), None);
        }
        assert!(PAGES.iter().all(|p| !p.2.is_empty()));
        assert_eq!(sound(false), "sound");
        assert_eq!(sound(true), "audio");
    }

    #[test]
    fn pages_step_round() {
        assert_eq!(step(false, "sound", 1), "timing");
        let last = pages(false).last().unwrap().0;
        assert_eq!(step(false, "sound", -1), last);
        assert_eq!(step(false, last, 1), "sound");
        assert_eq!(step(true, "library", 1), "audio");
    }

    #[test]
    fn keys_scroll_and_switch_pages() {
        let none = Modifiers::NONE;
        let ctrl = Modifiers::COMMAND;
        let shift = Modifiers::SHIFT;
        assert_eq!(nav_key(Key::Escape, none), Some(Nav::Close));
        assert_eq!(nav_key(Key::Backspace, none), None);
        assert_eq!(nav_key(Key::ArrowDown, none), Some(Nav::Scroll(60.0)));
        assert_eq!(nav_key(Key::ArrowUp, none), Some(Nav::Scroll(-60.0)));
        assert_eq!(nav_key(Key::ArrowDown, ctrl), Some(Nav::Page(1)));
        assert_eq!(nav_key(Key::ArrowUp, ctrl), Some(Nav::Page(-1)));
        assert_eq!(nav_key(Key::Tab, none), Some(Nav::Page(1)));
        assert_eq!(nav_key(Key::Tab, shift), Some(Nav::Page(-1)));
        assert_eq!(nav_key(Key::PageDown, none), Some(Nav::Scroll(400.0)));
        assert_eq!(nav_key(Key::M, none), None);
    }

    #[test]
    fn folds_open_and_close() {
        let mut f = Folds::default();
        assert!(!f.is_open("trim"));
        f.toggle("trim");
        assert!(f.is_open("trim"));
        f.open("trim");
        f.open("keys");
        assert!(f.is_open("keys"));
        f.toggle("trim");
        assert!(!f.is_open("trim"));
        assert!(f.is_open("keys"));
        f.close("keys");
        assert_eq!(f, Folds::default());
    }
}
