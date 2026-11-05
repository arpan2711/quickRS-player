//! The amp room (PLAN.md P22.2-P22.3, D47): plug in and play through the pedalboard with no song. Reached from the
//! home screen's AMP ROOM button (key A). From the owner-approved mockups (Room-Main, Room-Library, Room-Save). New
//! code (no Python source).
//!
//! The rig is your own board (config.json `tone_board`, the player's MINE), played and edited with the
//! pedalboard's own UI (`player::board::Rack`, shared with the player): the stage is its board, the editor its
//! editor. The room adds the selected unit's knobs, a gear shelf, the strip (input, gate, tuner, scenes, rig
//! stepping, master), scenes on keys 1-4, your saved sounds (`rigs`), and the Sounds tab. While you're in it the
//! engine plays the board and hears the guitar; leaving puts the tone and monitoring back as they were.
//!
//! R (or the Record key) records what you hear to a file, R pauses, Shift R stops and saves (`crate::recording`, P23.9); leaving the
//! room stops and saves it. The Rig tab's key is G.

mod catalogue;
pub(crate) mod draw;
pub mod noise;
pub(crate) mod plain;
pub mod rig;
pub mod rigs;
mod save;
mod sounds;
mod stage;
mod strip;
#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, Event, Key, Pos2, Rect, Ui, pos2, vec2};
use qrs_audio::board::{self, Board, Kind};
use qrs_audio::tone::ToneSpec;
use qrs_library::Config;
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets;

use crate::listen::Listen;
use crate::pedalboard as pb;
use crate::player::board::{Pick, Rack};
use crate::state::AppState;
use draw::KeyLook;
use rigs::{Rig, RoomState, SCENES};

/// The room's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Rig,
    Sounds,
}

/// Where typed text goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Typing {
    Shelf,
    Sounds,
    SaveName,
    /// A scene's name (double-click on it in the strip).
    Scene(usize),
}

/// The compare key (the one left of 1) in words (P41.3, D172): its glyph, a lone backtick, read as a stray mark in
/// the room's caps and toasts, so no text of the room prints it (`no_text_prints_the_backtick`).
pub(crate) const AB_KEY: &str = "the key left of 1";
/// A toast's tail naming it.
const AB_TAIL: &str = "  \u{b7}  the key left of 1 flips back";

/// The rig as the home screen's button names it: the saved sound it is, else its chain ("Perspex 100 · Mean
/// Green · 2 more").
pub fn rig_label(cfg: &Config) -> String {
    let st = RoomState::load(cfg);
    if let Some(n) = st.rig {
        return n;
    }
    let b = pb::my_board(cfg);
    let mut names: Vec<&str> = b
        .blocks
        .iter()
        .filter(|x| matches!(x.def().kind, Kind::Amp | Kind::Drive | Kind::Fuzz))
        .map(|x| x.def().name)
        .collect();
    if names.is_empty() {
        names = b.blocks.iter().map(|x| x.def().name).collect();
    }
    let more = b.blocks.len().saturating_sub(names.len().min(2));
    let mut s = names.iter().take(2).copied().collect::<Vec<_>>().join(" \u{b7} ");
    if more > 0 {
        s.push_str(&format!(" \u{b7} {more} more"));
    }
    if s.is_empty() { "Your rig".into() } else { s }
}

pub struct RoomView {
    pub rack: Rack,
    pub listen: Listen,
    pub tab: Tab,
    pub state: RoomState,
    /// Your saved sounds (read again when the user data changes).
    saved: Vec<Rig>,
    saved_ver: u64,
    /// What the engine had when the room opened (put back on leaving).
    tone_before: Option<ToneSpec>,
    monitor_before: Option<bool>,
    toast: Option<(String, String, Instant)>,
    /// The toast says a unit was removed: a click on it undoes that (P41.3).
    toast_undo: bool,
    /// A/B: hearing the saved sound instead of the rig as it is now.
    pub ab: bool,
    /// The shelf's family, its search, and where it adds (after the selected unit unless a + said where).
    pub shelf_kind: Kind,
    pub shelf_query: String,
    shelf_at: Option<usize>,
    /// The shelf's page (its cells don't all fit in a row: hundreds of NAM captures, P33.6).
    pub shelf_page: usize,
    typing: Option<Typing>,
    pub save: Option<save::Sheet>,
    pub sounds: sounds::Sounds,
    dice: rigs::Dice,
    pub closed: bool,
    /// Screenshots: hold still (no engine, no clock).
    pub shot: bool,
    area: Rect,
    /// The wheel's places this frame (the selected card's knobs; the strip's trim, gate and master).
    spots: Vec<stage::Spot>,
    strip_spots: Vec<(Rect, strip::What)>,
    /// When the wheel last turned one of the room's own knobs (the edit is kept once it rests).
    wheel_at: Option<Instant>,
    /// The Record key (the recording's badge hangs under it).
    rec_at: Rect,
    /// The one screen's own state (the drawer, quick controls, song tone explained: `rig`).
    pub rig_ui: rig::RigUi,
    /// A strip knob being dragged: (which, where it started, its turn there).
    strip_drag: Option<(&'static str, f32, f32)>,
    /// Settings asked for (the strip's Calibrate...): the app opens that page over the room.
    pub want_settings: Option<&'static str>,
    /// Opened from a song's Pedalboard tile: the song's tones are here (SONG TONE), and leaving goes back to it.
    pub song: bool,
    /// The Catalogue page over the rig (P37.13, D127), and the "i" dot's info card over the room.
    pub cat: Option<catalogue::Cat>,
    pub info: Option<stage::Entry>,
    /// Where the recording badge's right end goes this frame (the chain card's header), when there's a chain.
    pub badge_at: Option<Pos2>,
    /// The strip's "Learn noise" listening (P41.1).
    pub noise: noise::LearnNoise,
}

impl RoomView {
    /// Open the room: the engine plays your board and hears the guitar.
    pub fn open(st: &mut AppState) -> RoomView {
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        let mut rack = Rack::new(engine.clone());
        rack.board.pick = Some(Pick::Mine);
        rack.board.open = true;
        rack.board.stage = true;
        let state = RoomState::load(&st.config);
        let mut v = RoomView {
            rack,
            listen: Listen::default(),
            tab: Tab::Rig,
            state,
            saved: Vec::new(),
            saved_ver: u64::MAX,
            tone_before: None,
            monitor_before: None,
            toast: None,
            noise: noise::LearnNoise::default(),
            toast_undo: false,
            ab: false,
            shelf_kind: Kind::Drive,
            shelf_query: String::new(),
            shelf_at: None,
            shelf_page: 0,
            typing: None,
            save: None,
            sounds: sounds::Sounds::default(),
            dice: rigs::Dice::from_clock(),
            closed: false,
            shot: false,
            area: Rect::NOTHING,
            spots: Vec::new(),
            strip_spots: Vec::new(),
            wheel_at: None,
            rec_at: Rect::NOTHING,
            rig_ui: rig::RigUi::default(),
            cat: None,
            info: None,
            badge_at: None,
            strip_drag: None,
            want_settings: None,
            song: false,
        };
        if let Some(e) = &engine {
            v.tone_before = Some(e.tone());
            v.monitor_before = Some(e.monitor());
            e.set_monitor(true);
            e.set_tone(ToneSpec::Board {
                board: pb::my_board(&st.config),
            });
        }
        v.refresh_saved(st);
        // (no sounds of your own: the drawer opens on the starters)
        if v.saved.is_empty() {
            v.rig_ui.seg = rig::Seg::Starters;
        }
        // (P37.6c: the first time, with no sounds of your own yet, the room opens on its Sounds drawer: picking a
        // sound by ear before any knob)
        if st.config.get("room_seen").is_none() {
            // (P41.3: and the starters first in it, as big cards over the song tones)
            v.sounds.starters_first = true;
            if v.saved.is_empty() && st.audio_mode != crate::state::AudioMode::Shots {
                v.go(st, Tab::Sounds);
            }
            st.config.set("room_seen", serde_json::json!(true));
            st.save_config();
        }
        v.rack.title = Some(v.title());
        // the songs' tones for the Sounds tab (read in the background, once a session)
        if st.audio_mode != crate::state::AudioMode::Shots {
            crate::gearindex::start(
                st.songs.clone(),
                st.keys.clone(),
                st.data.cache().join("gear.json"),
            );
        }
        v
    }

    /// The room in a song's context (the player's Pedalboard tile, D102): the song's tones beside your sounds, the
    /// tone the player had playing.
    pub fn open_song(
        st: &mut AppState,
        song: crate::player::board::SongCtx,
        pick: Option<Pick>,
        song_tone: usize,
    ) -> RoomView {
        let mut v = RoomView::open(st);
        v.song = true;
        v.rack.for_song(st, song, pick, song_tone);
        v.rig_ui.seg = rig::Seg::Song;
        v.rack.board_send(st);
        v
    }

    /// Leave: the edits kept, the room's state saved, the engine's tone and monitoring as they were.
    pub fn leave(&mut self, st: &mut AppState) {
        self.rack.board.audition = None;
        self.rack.board_commit(st);
        if self.source_now(st) != Source::Song {
            self.keep_scene(st);
        }
        self.state.save(&mut st.config);
        if let Some(e) = self.rack.engine().cloned() {
            match self.tone_before.take() {
                // (the board the engine had is your board as it was: it's your board as it is now)
                Some(ToneSpec::Board { .. }) | None => e.set_tone(ToneSpec::Board {
                    board: pb::my_board(&st.config),
                }),
                Some(t) => e.set_tone(t),
            }
            if let Some(m) = self.monitor_before.take() {
                e.set_monitor(m);
            }
        }
        st.save_config();
    }

    fn refresh_saved(&mut self, st: &AppState) {
        if self.saved_ver != st.userdata.version {
            self.saved_ver = st.userdata.version;
            self.saved = rigs::saved(&st.userdata);
        }
    }

    /// The saved sound the rig is.
    fn saved_rig(&self) -> Option<&Rig> {
        let n = self.state.rig.as_ref()?;
        self.saved.iter().find(|r| r.name.eq_ignore_ascii_case(n))
    }

    /// The rig's name: the saved sound, else what it's based on, else "My rig".
    pub fn title(&self) -> String {
        if let Some(n) = &self.state.rig {
            return n.clone();
        }
        if !self.state.based_on.is_empty() {
            return self.state.based_on.clone();
        }
        "My rig".into()
    }

    /// The rig differs from the saved sound (or isn't one).
    fn edited(&mut self, st: &AppState) -> bool {
        let now = self.rack.board_now(st);
        match self.saved_rig() {
            Some(r) => !rigs::same_sound(&r.board, &now),
            None => true,
        }
    }

    pub fn say(&mut self, text: impl Into<String>, tail: impl Into<String>) {
        self.toast_undo = false;
        self.toast = Some((
            text.into(),
            tail.into(),
            Instant::now() + Duration::from_millis(1800),
        ));
    }

    // -- the rig ------------------------------------------------------------------------------------------------------

    /// Put a whole rig on (a saved sound, a starting rig, a song's tone): undo has the one before.
    pub fn load(
        &mut self,
        st: &mut AppState,
        b: Board,
        scenes: [Option<Board>; SCENES],
        rig: Option<String>,
        based_on: String,
    ) {
        // (P38.2: a capture whose file is gone gives way to a model of ours, never the guitar dry)
        let mut b = b;
        let healed = crate::captures::heal(&mut b);
        self.ab = false;
        self.rack.board.audition = None;
        // (a whole rig goes on your sound, never over a song's tone)
        self.rack.board.pick = Some(Pick::Mine);
        let name = rig.clone().unwrap_or_else(|| based_on.clone());
        self.rack.load_board(st, &name, b);
        self.rack.said.clear();
        self.state.rig = rig;
        self.state.based_on = based_on;
        self.state.scenes = scenes;
        self.state.scene = 0;
        // the first scene the sound has, if it has one, is what plays
        if let Some(s) = self.state.scenes[0].clone() {
            let cur = self.rack.board_now(st);
            let out = rigs::scene_board(&cur, &s);
            self.rack.board_edit(st, |x| *x = out);
            self.rack.board_commit(st);
        }
        self.rack.board.sel = None;
        self.rack.title = Some(self.title());
        if healed > 0 {
            self.rack.say(
                "A capture's file is missing",
                "  \u{b7}  playing the model of ours in its place",
            );
        }
        self.state.save(&mut st.config);
        st.save_config();
        self.say(format!("{name} on"), "  \u{b7}  Ctrl Z takes it back");
    }

    pub fn load_saved(&mut self, st: &mut AppState, r: &Rig) {
        self.load(
            st,
            r.board.clone(),
            r.scenes.clone(),
            Some(r.name.clone()),
            r.based_on.clone(),
        );
    }

    /// The playing scene's settings into it (before switching away, saving, leaving).
    fn keep_scene(&mut self, st: &AppState) {
        let cur = self.rack.board_now(st);
        self.state.scenes[self.state.scene] = Some(cur);
    }

    /// Scene `n` (0-3): its on/off and knobs on the rig, in place (no gap: the engine changes knobs and bypasses).
    pub fn scene(&mut self, st: &mut AppState, n: usize) {
        if n >= SCENES || n == self.state.scene {
            return;
        }
        self.rack.board_commit(st);
        self.keep_scene(st);
        if let Some(s) = self.state.scenes[n].clone() {
            let cur = self.rack.board_now(st);
            let out = rigs::scene_board(&cur, &s);
            self.rack.board_edit(st, |x| *x = out);
            self.rack.board_commit(st);
        }
        self.state.scene = n;
        self.state.save(&mut st.config);
        st.save_config();
        self.rack.said.clear();
    }

    /// Save the rig as the sound it is (Ctrl S); a rig that isn't one yet opens the save sheet.
    pub fn save_now(&mut self, st: &mut AppState) {
        let Some(mut r) = self.saved_rig().cloned() else {
            self.open_save(st);
            return;
        };
        self.rack.board_commit(st);
        self.keep_scene(st);
        r.board = self.rack.board_now(st);
        r.scenes = self.state.scenes.clone();
        r.saved = now_secs();
        rigs::store(&mut st.userdata, &r);
        self.refresh_saved(st);
        self.say(format!("Saved \u{201c}{}\u{201d}", r.name), "");
    }

    pub fn open_save(&mut self, st: &mut AppState) {
        self.rack.board_commit(st);
        self.keep_scene(st);
        let base = self.saved_rig().cloned();
        let now = self.rack.board_now(st);
        self.save = Some(save::Sheet::new(
            &self.title(),
            base,
            &self.state.based_on,
            &self.saved,
            &now,
        ));
        self.typing = Some(Typing::SaveName);
    }

    pub(super) fn shuffle(&mut self, st: &mut AppState) {
        let b = rigs::shuffle(&mut self.dice);
        let name = format!("Shuffle: {}", rigs::shuffle_name(&b));
        self.load(st, b, Default::default(), None, name);
    }

    /// Every rig you can step through: your sounds, then the starting rigs.
    fn rig_list(&self) -> Vec<(String, Board, Option<Rig>)> {
        let mut v: Vec<(String, Board, Option<Rig>)> = self
            .saved
            .iter()
            .map(|r| (r.name.clone(), r.board.clone(), Some(r.clone())))
            .collect();
        for (n, b) in board::presets() {
            v.push((n.to_string(), b, None));
        }
        v
    }

    /// Where the rig is in that list.
    fn rig_index(&self, list: &[(String, Board, Option<Rig>)]) -> Option<usize> {
        let name = self
            .state
            .rig
            .clone()
            .unwrap_or_else(|| self.state.based_on.clone());
        list.iter().position(|(n, _, saved)| {
            n.eq_ignore_ascii_case(&name) && (saved.is_some() == self.state.rig.is_some())
        })
    }

    fn step_rig(&mut self, st: &mut AppState, dir: i32) {
        let list = self.rig_list();
        if list.is_empty() {
            return;
        }
        let n = list.len() as i32;
        let i = match self.rig_index(&list) {
            Some(i) => (i as i32 + dir).rem_euclid(n),
            None => {
                if dir > 0 {
                    0
                } else {
                    n - 1
                }
            }
        } as usize;
        let (name, b, saved) = list[i].clone();
        match saved {
            Some(r) => self.load_saved(st, &r),
            None => self.load(st, b, Default::default(), None, name),
        }
    }

    /// The Catalogue page on or off (the C key, the top line's key).
    pub fn toggle_catalogue(&mut self, st: &mut AppState) {
        if self.cat.take().is_none() {
            self.go(st, Tab::Rig);
            self.cat = Some(catalogue::Cat::new(st.instrument().is_bass()));
        }
    }

    /// The Catalogue page found on a capture by its name (the downloads card's Open in the amp room, P38.3).
    pub fn show_capture(&mut self, st: &mut AppState, name: &str) {
        self.go(st, Tab::Rig);
        let mut c = catalogue::Cat::new(st.instrument().is_bass());
        c.query = name.to_string();
        self.cat = Some(c);
    }

    fn toggle_ab(&mut self, st: &mut AppState) {
        // a song's tone: A/B against RS's original (the tone before your edits)
        if self.source_now(st) == Source::Song {
            if self.rack.board.audition.take().is_some() {
                self.say("Hearing the tone as it is now", AB_TAIL);
            } else {
                let i = match self.rack.board.pick {
                    Some(Pick::Tone(i)) => i,
                    _ => self.rack.board.song_tone,
                };
                let orig = crate::pedalboard::SongRig::new(
                    &self.rack.song.arr,
                    &self.rack.song.sid,
                    &self.rack.song.part_name,
                    &qrs_library::UserData::default(),
                )
                .and_then(|r| r.tones.get(i).map(|t| t.board.clone()));
                match orig {
                    Some(b) => {
                        self.rack.board.audition = Some(b.renewed());
                        self.say("Hearing the song's original tone", AB_TAIL);
                    }
                    None => self.say("No original to compare with", ""),
                }
            }
            self.rack.board_send(st);
            return;
        }
        let Some(r) = self.saved_rig().cloned() else {
            self.say("Nothing saved to compare with", "  \u{b7}  save the rig first");
            return;
        };
        self.ab = !self.ab;
        self.rack.board.audition = if self.ab {
            let mut b = r.board.clone();
            if let Some(s) = &r.scenes[self.state.scene] {
                b = rigs::scene_board(&b, s);
            }
            Some(b.renewed())
        } else {
            None
        };
        self.rack.board_send(st);
        self.say(
            if self.ab {
                "Hearing the saved sound"
            } else {
                "Hearing your rig"
            },
            AB_TAIL,
        );
    }

    // -- keys ---------------------------------------------------------------------------------------------------------

    pub fn handle(&mut self, st: &mut AppState, ev: &Event) {
        // the info card, then the Catalogue, take the keys while they're open
        if self.info.is_some() {
            catalogue::info_handle(self, ev);
            return;
        }
        if self.cat.is_some() && self.save.is_none() && !self.rack.board.editing && self.tab == Tab::Rig {
            catalogue::handle(self, st, ev);
            return;
        }
        // typing first
        if let Some(t) = self.typing {
            match ev {
                Event::Text(s) => {
                    let s = s.clone();
                    match t {
                        Typing::Shelf => push_text(&mut self.shelf_query, &s, 24),
                        Typing::Sounds => {
                            push_text(&mut self.sounds.query, &s, 40);
                            self.sounds.moved();
                        }
                        Typing::SaveName => {
                            if let Some(sh) = &mut self.save {
                                push_text(&mut sh.name, &s, 40);
                            }
                        }
                        Typing::Scene(n) => push_text(&mut self.state.names[n], &s, 24),
                    }
                    return;
                }
                Event::Key {
                    key: Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    match t {
                        Typing::Shelf => {
                            self.shelf_query.pop();
                        }
                        Typing::Sounds => {
                            self.sounds.query.pop();
                            self.sounds.moved();
                        }
                        Typing::SaveName => {
                            if let Some(sh) = &mut self.save {
                                sh.name.pop();
                            }
                        }
                        Typing::Scene(n) => {
                            self.state.names[n].pop();
                        }
                    }
                    return;
                }
                // (letters and digits typed are the text's, not keys)
                Event::Key { key, modifiers, .. }
                    if !matches!(key, Key::Escape | Key::Enter | Key::ArrowUp | Key::ArrowDown)
                        && !modifiers.command =>
                {
                    return;
                }
                _ => {}
            }
        }
        if let Event::MouseWheel {
            unit,
            delta,
            modifiers,
            ..
        } = ev
        {
            let notches = crate::library::wheel_notches(*unit, delta.y);
            let pos = st.ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
            if self.tab == Tab::Rig && self.rack.board_wheel(st, pos, notches, modifiers.shift) {
                return;
            }
            if self.wheel(st, pos, notches, modifiers.shift) {
                return;
            }
            if self.tab == Tab::Sounds {
                self.sounds.wheel(pos, notches);
            }
            return;
        }
        let Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = ev
        else {
            return;
        };
        let ctrl = modifiers.command;
        // the save sheet is modal
        if self.save.is_some() {
            match key {
                Key::Escape => {
                    self.save = None;
                    self.typing = None;
                }
                Key::Enter => self.finish_save(st),
                _ => {}
            }
            return;
        }
        if self.rack.board.editing {
            if *key == Key::Escape {
                self.rack.board_commit(st);
                self.rack.board.editing = false;
            } else if ctrl && *key == Key::Z {
                self.rack.undo_edit(st);
            } else if ctrl && *key == Key::Y {
                self.rack.redo_edit(st);
            }
            return;
        }
        if let Some(Typing::Scene(_)) = self.typing
            && matches!(key, Key::Enter | Key::Escape)
        {
            self.typing = None;
            self.state.save(&mut st.config);
            st.save_config();
            return;
        }
        match key {
            Key::Escape => {
                if self.typing.is_some() {
                    self.typing = None;
                    if self.tab == Tab::Sounds && !self.sounds.query.is_empty() {
                        self.sounds.query.clear();
                        self.sounds.moved();
                    }
                } else if self.tab == Tab::Sounds && self.sounds.previewing() {
                    self.sounds.stop(&mut self.rack, st);
                    self.say("Back to your rig", "");
                } else if self.tab != Tab::Rig {
                    self.go(st, Tab::Rig);
                } else if self.shelf_at.is_some() {
                    self.shelf_at = None;
                } else {
                    self.closed = true;
                }
            }
            Key::S if ctrl && modifiers.shift => self.open_save(st),
            Key::S if ctrl => self.save_now(st),
            Key::Z if ctrl => self.rack.undo_edit(st),
            Key::Y if ctrl => self.rack.redo_edit(st),
            // R records what you hear, pauses, resumes; Shift R stops and saves (P23.9); the Rig tab is G
            Key::R if !ctrl => {
                let said = if modifiers.shift {
                    crate::recording::finish(st)
                } else {
                    crate::recording::press(st, crate::recording::Page::Room, &self.title())
                };
                if let Some((a, b)) = said {
                    self.say(a, b);
                }
            }
            Key::G if !ctrl => self.go(st, Tab::Rig),
            Key::L if !ctrl => self.go(st, Tab::Sounds),
            Key::F if ctrl => {
                self.go(st, Tab::Sounds);
                self.typing = Some(Typing::Sounds);
            }
            Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4 => {
                let n = match key {
                    Key::Num1 => 0,
                    Key::Num2 => 1,
                    Key::Num3 => 2,
                    _ => 3,
                };
                if self.tab == Tab::Sounds && self.sounds.previewing() && modifiers.shift {
                    sounds::load_into_scene(self, st, n);
                } else {
                    self.scene(st, n);
                }
            }
            Key::Backtick => self.toggle_ab(st),
            Key::C if !ctrl => self.toggle_catalogue(st),
            Key::E if self.tab == Tab::Rig && !ctrl => {
                if self.rack.board.sel.is_some() {
                    self.rack.board.editing = true;
                }
            }
            Key::Tab if self.tab == Tab::Rig => {
                let b = self.rack.board_now(st);
                if !b.blocks.is_empty() {
                    let i = self.rack.board.sel.and_then(|u| b.index_of(u)).unwrap_or(0);
                    let n = b.blocks.len();
                    let j = if modifiers.shift {
                        (i + n - 1) % n
                    } else {
                        (i + 1) % n
                    };
                    self.rack.board.sel = Some(b.blocks[j].uid);
                }
            }
            Key::Space if self.tab == Tab::Rig => {
                if let Some(u) = self.rack.board.sel {
                    self.rack.toggle_block(st, u);
                }
            }
            // (Page Up / Down step the rigs; the arrows used to, and threw unsaved edits away: B13)
            Key::PageUp if self.tab == Tab::Rig => self.step_rig(st, -1),
            Key::PageDown if self.tab == Tab::Rig => self.step_rig(st, 1),
            _ if self.tab == Tab::Sounds => sounds::key(self, st, *key, *modifiers),
            _ => {}
        }
        self.rack.said.clear();
    }

    pub fn go(&mut self, st: &mut AppState, tab: Tab) {
        if self.tab == Tab::Sounds && tab != Tab::Sounds && self.sounds.previewing() {
            self.sounds.stop(&mut self.rack, st);
        }
        self.tab = tab;
        self.typing = None;
        if tab == Tab::Sounds {
            self.sounds.moved();
        }
    }

    fn finish_save(&mut self, st: &mut AppState) {
        let Some(sh) = self.save.take() else { return };
        self.typing = None;
        let name = sh.name.trim().to_string();
        if name.is_empty() {
            self.say("A sound needs a name", "");
            self.save = Some(sh);
            self.typing = Some(Typing::SaveName);
            return;
        }
        self.keep_scene(st);
        let r = Rig {
            name: if sh.overwrite {
                sh.base.as_ref().map(|b| b.name.clone()).unwrap_or(name.clone())
            } else {
                name.clone()
            },
            tags: sh.tags.clone(),
            color: sh.color,
            slot: sh.slot,
            based_on: sh.based_on.clone(),
            board: self.rack.board_now(st),
            scenes: self.state.scenes.clone(),
            saved: now_secs(),
        };
        rigs::store(&mut st.userdata, &r);
        self.refresh_saved(st);
        self.state.rig = Some(r.name.clone());
        self.state.based_on = r.based_on.clone();
        self.state.save(&mut st.config);
        st.save_config();
        self.rack.title = Some(self.title());
        self.say(
            format!("Saved \u{201c}{}\u{201d}", r.name),
            "  \u{b7}  in your sounds",
        );
    }

    /// The wheel over the room's own controls (the selected unit's knobs, the gate, the trim, the master).
    fn wheel(&mut self, st: &mut AppState, pos: Pos2, notches: f32, shift: bool) -> bool {
        if self.tab == Tab::Rig
            && (stage::wheel(self, st, pos, notches, shift) || rig::wheel(self, st, pos, notches, shift))
        {
            return true;
        }
        strip::wheel(self, st, pos, notches, shift)
    }

    // -- drawing ------------------------------------------------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui, st: &mut AppState, area: Rect) {
        self.area = area;
        self.refresh_saved(st);
        let p = ui.painter().clone();
        let th = t();
        p.rect_filled(area, 0.0, th.base);
        // the engine in step: the board sent when it changes, edits kept when the mouse lets go
        self.rack.engine = st.audio.as_ref().map(|a| a.engine.clone());
        self.rack.title = Some(self.title());
        self.rack.begin_frame(ui.ctx());
        self.rack.board_update(st);
        self.listen.set_instrument(st.instrument());
        let live = self.listen.update(self.rack.engine.as_deref());
        if live || self.rack.board.editing {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        } else {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        self.sounds.poll(st);
        // a wheel turned on the room's own knobs: kept once it rests
        if self
            .wheel_at
            .is_some_and(|w| w.elapsed() > Duration::from_millis(700))
        {
            self.wheel_at = None;
            self.rack.board_commit(st);
        }
        let (top, body, strip_r) = room_rects(area);
        self.top_bar(ui, st, top);
        // (the editor over the stage: the stage is drawn only, B1/B4)
        self.rack.board.inert = self.rack.board.editing && self.tab == Tab::Rig;
        match self.tab {
            Tab::Rig if self.cat.is_some() => catalogue::page(self, ui, st, body),
            Tab::Rig => rig::page(self, ui, st, body),
            Tab::Sounds => sounds::page(self, ui, st, body),
        }
        noise::update(self, st);
        strip::strip(self, ui, st, strip_r);
        // the detailed editor, over the stage and cards
        self.rack.board.inert = false;
        if self.rack.board.editing && self.tab == Tab::Rig {
            paint::dim(&p, area, Some(150));
            let panel = body;
            p.rect_filled(panel, 24.0, th.base);
            paint::floating(&p, panel, Some(24.0));
            self.rack.editor_view(ui, st, panel.shrink(px(20.0)), 0.0);
            if !self.rack.board.open {
                // (its close key: back to the room)
                self.rack.board.open = true;
                self.rack.board.editing = false;
            }
        }
        // the captures dialog (NAM captures and IR cabs, P31), over the room
        self.rack.captures_ui(ui, st, area);
        catalogue::info_card(self, ui, area);
        if let Some(sh) = &mut self.save {
            let act = sh.ui(ui, st, area);
            match act {
                save::Act::None => {}
                save::Act::Cancel => {
                    self.save = None;
                    self.typing = None;
                }
                save::Act::Save => self.finish_save(st),
                save::Act::Type => self.typing = Some(Typing::SaveName),
            }
        }
        // recording: the lamp, time and keys under the Record key (a click stops it)
        if let Some(s) = st.rec.status() {
            let w = crate::recording::badge_width(ui, s);
            // (in the chain card's header at its right, P37.13: hung under the key it sat on the card's edge)
            let at = match self.badge_at.take() {
                Some(right) => pos2(right.x - w, right.y),
                None => pos2(
                    (self.rec_at.center().x - w / 2.0).min(area.max.x - px(20.0) - w),
                    body.min.y + px(20.0),
                ),
            };
            if crate::recording::badge(ui, at, s).clicked()
                && let Some((a, b)) = crate::recording::toggle(st, crate::recording::Page::Room, "")
            {
                self.say(a, b);
            }
        }
        // what the rack said, as the room's toast; its tooltip (its requests are the player's: none here)
        self.rack.reqs.clear();
        for (a, b) in std::mem::take(&mut self.rack.said) {
            // (a unit removed: "Removed Mean Green · Undo", the toast itself the undo, longer on screen, P41.3)
            let removed = a.starts_with("Removed ") && self.rack.can_undo();
            self.say(a, b);
            if removed {
                self.toast_undo = true;
                if let Some(t) = &mut self.toast {
                    t.2 = Instant::now() + Duration::from_millis(4500);
                }
            }
        }
        if let Some((msg, tail, until)) = self.toast.clone() {
            if Instant::now() < until {
                let at = pos2(body.center().x, body.max.y - px(40.0));
                widgets::toast(&p, &msg, at, &tail);
                if self.toast_undo {
                    let ts = Style::new(16.0, 700, th.text);
                    let w = text::width(ui.ctx(), &msg, ts) + text::width(ui.ctx(), &tail, ts) + px(44.0);
                    let tr = Rect::from_center_size(at, vec2(w, px(46.0)));
                    let resp = widgets::interact(ui, tr, "room-toast-undo", egui::Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.rack.undo_edit(st);
                        self.toast = None;
                        self.toast_undo = false;
                    }
                }
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            } else {
                self.toast = None;
                self.toast_undo = false;
            }
        }
        if let Some((text_, key, pos, anchor)) = self.rack.tip.take() {
            crate::player::draw_tip(&p, &text_, &key, pos, anchor, area);
        }
    }

    /// Where the sound now comes from (the top line's chips).
    fn source_now(&mut self, st: &AppState) -> Source {
        if self.rack.board.rig.is_some() && self.rack.board.pick != Some(Pick::Mine) {
            return Source::Song;
        }
        if self.rack.board_now(st).blocks.is_empty() {
            Source::Blank
        } else {
            Source::Mine
        }
    }

    /// The top line says what you hear and where it came from (P37.1, D102): the song's tone (which, of how many),
    /// your sound or a blank rig; edited; A/B against the original; Save as; Save (the room's one bright key).
    fn top_bar(&mut self, ui: &Ui, st: &mut AppState, r: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let h = r.height();
        let home = Rect::from_min_size(r.min, vec2(px(44.0), h));
        if draw::key(
            ui,
            home,
            "room-home",
            KeyLook::Dark,
            "",
            "",
            Some(&|p: &egui::Painter, c: Pos2, s: f32, col: Color32| draw::icon_chev(p, c, s, col, -1.0)),
        )
        .clicked()
        {
            self.closed = true;
        }
        // the right group: undo, redo, Record, A/B, Save as, Save
        let mut x = r.max.x;
        let place = |x: &mut f32, w: f32| {
            let rr = Rect::from_min_size(pos2(*x - w, r.min.y), vec2(w, h));
            *x = rr.min.x - px(8.0);
            rr
        };
        let save_r = place(&mut x, draw::key_w(&ctx, "Save", "Ctrl S", true));
        let saveas_r = place(&mut x, draw::key_w(&ctx, "Save as\u{2026}", "", false));
        let src = self.source_now(st);
        // (A/B: A is the sound as you're changing it, B the saved sound or the song's original)
        let b_word = if src == Source::Song {
            "Original"
        } else {
            "Last saved"
        };
        let ab_r = place(&mut x, draw::ab_w(&ctx, b_word));
        x -= px(6.0);
        let recording = st.rec.status();
        let rec_r = place(&mut x, h);
        self.rec_at = rec_r;
        let redo_r = place(&mut x, h);
        let undo_r = place(&mut x, h);
        x -= px(6.0);
        // the catalogue's key says how much is in it, so it's noticed (P41.5, owner request)
        let cat_label = format!("Catalogue · {} units", catalogue::unit_count());
        let cat_r = place(&mut x, draw::key_w(&ctx, &cat_label, "C", true));
        let cat_on = self.cat.is_some();
        let grid = |p: &egui::Painter, c: Pos2, s: f32, col: Color32| draw::icon_grid(p, c, s, col);
        let cat = draw::key(
            ui,
            cat_r,
            "room-catalogue",
            if cat_on { KeyLook::Lit } else { KeyLook::Dark },
            &cat_label,
            "C",
            Some(&grid),
        );
        if cat.hovered() {
            self.rack.tip = Some((
                "Every amp, cab, pedal and capture you have, to find and add".into(),
                "C".into(),
                pos2(cat_r.center().x, cat_r.max.y + px(4.0)),
                Align2::CENTER_TOP,
            ));
        }
        if cat.clicked() {
            self.toggle_catalogue(st);
        }
        if draw::key(
            ui,
            save_r,
            "room-save",
            KeyLook::Amber,
            "Save",
            "Ctrl S",
            Some(&draw::icon_save),
        )
        .clicked()
        {
            self.save_now(st);
        }
        if draw::key(
            ui,
            saveas_r,
            "room-saveas",
            KeyLook::Dark,
            "Save as\u{2026}",
            "",
            None,
        )
        .clicked()
        {
            self.open_save(st);
        }
        let ab_on = self.ab || (src == Source::Song && self.rack.board.audition.is_some());
        let ab = draw::ab_switch(ui, ab_r, "room-ab", b_word, ab_on);
        if ab.hovered() {
            self.rack.tip = Some((
                if src == Source::Song {
                    "Compare: now (this tone as you've changed it) \u{b7} original (the song's own tone)"
                        .into()
                } else {
                    "Compare: now (your rig as it is) \u{b7} last saved (the sound as you saved it)".into()
                },
                format!("{AB_KEY} flips between them"),
                pos2(ab_r.center().x, ab_r.max.y + px(4.0)),
                Align2::CENTER_TOP,
            ));
        }
        if ab.clicked() {
            self.toggle_ab(st);
        }
        let dot =
            |p: &egui::Painter, c: Pos2, s: f32, _: Color32| crate::recording::icon_dot(p, c, s, t().miss);
        let rec = draw::key(
            ui,
            rec_r,
            "room-rec",
            if recording.is_some() {
                KeyLook::Rec
            } else {
                KeyLook::Dark
            },
            "",
            "",
            Some(&dot),
        );
        if rec.hovered() {
            self.rack.tip = Some((
                if recording.is_some() {
                    "Stop recording".into()
                } else {
                    "Record what you hear".into()
                },
                "R".into(),
                pos2(rec_r.center().x, rec_r.max.y + px(4.0)),
                Align2::CENTER_TOP,
            ));
        }
        if rec.clicked()
            && let Some((a, b)) = crate::recording::toggle(st, crate::recording::Page::Room, &self.title())
        {
            self.say(a, b);
        }
        let (can_undo, can_redo) = (self.rack.can_undo(), self.rack.can_redo());
        let redo = draw::key(
            ui,
            redo_r,
            "room-redo",
            KeyLook::Dark,
            "",
            "",
            Some(&draw::icon_redo),
        );
        if redo.clicked() && can_redo {
            self.rack.redo_edit(st);
        }
        let undo = draw::key(
            ui,
            undo_r,
            "room-undo",
            KeyLook::Dark,
            "",
            "",
            Some(&draw::icon_undo),
        );
        if undo.clicked() && can_undo {
            self.rack.undo_edit(st);
        }
        for (resp, rr, what, key, can) in [
            (&undo, undo_r, "Undo", "Ctrl Z", can_undo),
            (&redo, redo_r, "Redo", "Ctrl Y", can_redo),
        ] {
            if resp.hovered() {
                self.rack.tip = Some((
                    if can {
                        format!("{what} the last change")
                    } else {
                        format!("Nothing to {}", what.to_lowercase())
                    },
                    key.into(),
                    pos2(rr.center().x, rr.max.y + px(4.0)),
                    Align2::CENTER_TOP,
                ));
            }
        }
        if !can_undo {
            paint::wash(&p, undo_r, th.base, 0.45);
        }
        if !can_redo {
            paint::wash(&p, redo_r, th.base, 0.45);
        }
        // the sources: SONG TONE (in a song) | MY SOUND | BLANK, and "edited"
        let mut chips: Vec<(Source, &str)> = Vec::new();
        if self.rack.board.rig.is_some() {
            chips.push((Source::Song, "Song tone"));
        }
        chips.push((Source::Mine, "My sound"));
        chips.push((Source::Blank, "Blank"));
        let cw: f32 = chips
            .iter()
            .map(|c| draw::chip_w(&ctx, c.1, true, 11.5) + px(6.0))
            .sum();
        let edited = self.tab == Tab::Rig && self.edited(st) && self.rack.can_undo();
        let ew = if edited { px(110.0) } else { 0.0 };
        let cx0 = (cat_r.min.x - px(24.0) - ew - cw).max(r.min.x + px(420.0));
        let mut cx = cx0;
        if self.tab == Tab::Sounds {
            if link_key(
                ui,
                pos2(cx, r.center().y),
                "room-back-rig",
                "\u{2039} The rig (G)",
            )
            .clicked()
            {
                self.go(st, Tab::Rig);
            }
        } else {
            for (s, l) in &chips {
                let w = draw::chip_w(&ctx, l, true, 11.5);
                let cr = Rect::from_min_size(pos2(cx, r.center().y - px(14.0)), vec2(w, px(28.0)));
                cx = cr.max.x + px(6.0);
                if draw::chip(ui, cr, ("room-src", *l), l, None, src == *s, 11.5).clicked() && src != *s {
                    self.set_source(st, *s);
                }
            }
            if edited {
                text::draw(
                    &p,
                    "edited \u{b7} Ctrl Z",
                    pos2(cx + px(8.0), r.center().y),
                    Align2::LEFT_CENTER,
                    Style::new(11.5, 700, th.accent_ink),
                    None,
                );
            }
        }
        // the title: what you hear, and where it came from
        let tx0 = home.max.x + px(16.0);
        let room_w = cx0 - tx0 - px(16.0);
        let (sub, title) = self.what_you_hear(st, src);
        text::caps(
            &p,
            &sub,
            pos2(tx0, r.min.y + px(6.0)),
            10.5,
            th.caption,
            Align2::LEFT_CENTER,
            Some(room_w),
        );
        text::draw(
            &p,
            &title,
            pos2(tx0, r.min.y + px(29.0)),
            Align2::LEFT_CENTER,
            Style::new(20.0, 800, th.text),
            Some(room_w.max(px(60.0))),
        );
    }

    /// The top line's words: (the small caps over it, the line).
    fn what_you_hear(&mut self, st: &AppState, src: Source) -> (String, String) {
        if self.tab == Tab::Sounds && self.sounds.previewing() {
            return (
                "AMP ROOM  \u{b7}  SOUNDS  \u{b7}  PREVIEWING".into(),
                self.sounds.preview_name(),
            );
        }
        match src {
            Source::Song => {
                let rig = self.rack.board.rig.as_ref();
                let n = rig.map(|r| r.tones.len()).unwrap_or(0);
                let i = match self.rack.board.pick {
                    Some(Pick::Tone(i)) => i,
                    _ => self.rack.board.song_tone,
                }
                .min(n.saturating_sub(1));
                let (label, yours) = rig
                    .and_then(|r| r.tones.get(i))
                    .map(|t| (t.label.to_uppercase(), t.edited))
                    .unwrap_or_default();
                let how = if self.rack.board.pick.is_none() && crate::pedalboard::follow(&st.config) {
                    "FOLLOWING THE SONG"
                } else {
                    "HELD"
                };
                (
                    format!("AMP ROOM  \u{b7}  SONG TONE  \u{b7}  {how}"),
                    format!(
                        "{} \u{b7} {} \u{b7} song tone {} of {n} ({label}){}",
                        self.rack.song.display_title,
                        self.rack.song.part_name,
                        i + 1,
                        if yours { " \u{b7} yours" } else { "" }
                    ),
                )
            }
            Source::Mine => {
                let sub = match (&self.state.rig, self.state.based_on.is_empty()) {
                    (Some(_), true) => "AMP ROOM  \u{b7}  MY SOUND".into(),
                    (Some(_), false) => format!(
                        "AMP ROOM  \u{b7}  MY SOUND  \u{b7}  BASED ON {}",
                        self.state.based_on.to_uppercase()
                    ),
                    (None, false) => format!(
                        "AMP ROOM  \u{b7}  NOT SAVED  \u{b7}  BASED ON {}",
                        self.state.based_on.to_uppercase()
                    ),
                    (None, true) => "AMP ROOM  \u{b7}  NOT SAVED".into(),
                };
                let scene = rigs::scene_label(&self.state.names, self.state.scene);
                (sub, format!("{} \u{b7} {scene}", self.title()))
            }
            // (P38.1: what to do with it, in the small caps over its name)
            Source::Blank => (
                "AMP ROOM  \u{b7}  BLANK  \u{b7}  + IN THE CHAIN ADDS A UNIT".into(),
                "A blank rig".into(),
            ),
        }
    }

    /// Change where the sound comes from (the top line's chips).
    fn set_source(&mut self, st: &mut AppState, s: Source) {
        self.rack.board_commit(st);
        self.rack.board.audition = None;
        self.ab = false;
        match s {
            Source::Song => {
                self.rack.board.pick = None;
                self.rack.board.sel = None;
            }
            Source::Mine => {
                self.rack.board.pick = Some(Pick::Mine);
                self.rack.board.sel = None;
            }
            Source::Blank => {
                self.rack.board.pick = Some(Pick::Mine);
                self.load(st, Board::default(), Default::default(), None, "Blank".into());
            }
        }
    }
}

/// The room's three bands in `area` (P37.13): the top line, the body, the strip.
pub fn room_rects(area: Rect) -> (Rect, Rect, Rect) {
    let m = px(20.0);
    let top = Rect::from_min_size(
        pos2(area.min.x + m, area.min.y + px(18.0)),
        vec2(area.width() - 2.0 * m, px(44.0)),
    );
    let strip = Rect::from_min_max(
        pos2(area.min.x + m, area.max.y - px(12.0) - strip::HEIGHT),
        pos2(area.max.x - m, area.max.y - px(12.0)),
    );
    let body = Rect::from_min_max(
        pos2(area.min.x + m, top.max.y + px(22.0)),
        pos2(area.max.x - m, strip.min.y - px(10.0)),
    );
    (top, body, strip)
}

/// Where the room's sound comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Song,
    Mine,
    Blank,
}

/// A quiet text key in the top line.
fn link_key(ui: &Ui, pos: Pos2, id: &str, s: &str) -> egui::Response {
    let th = t();
    let st = Style::new(13.0, 800, th.accent_ink);
    let w = text::width(ui.ctx(), s, st);
    let r = Rect::from_min_size(pos2(pos.x, pos.y - px(12.0)), vec2(w, px(24.0)));
    let resp = widgets::interact(ui, r, id, egui::Sense::click());
    text::draw(ui.painter(), s, r.left_center(), Align2::LEFT_CENTER, st, None);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Screenshot mode: four saved sounds (in the favourite slots) and the room on one of them with an edit, as the
/// mockups; songs' tones from the library's first songs when none have been read.
pub fn stage_data(st: &mut AppState) {
    let b = |id: &str| board::Block::of(id).expect("catalogue model");
    let lead = Board::new(vec![
        b("squeeze").with("sustain", 4.0),
        b("green").with("drive", 3.8).with("tone", 5.5).with("level", 7.2),
        b("amp_crunch").with("gain", 7.0).with("treble", 6.0),
        b("cab_4x12"),
        {
            let mut x = b("phaser");
            x.on = false;
            x
        },
        b("analog_delay").with("mix", 22.0),
        b("spring").with("dwell", 4.0).with("mix", 20.0),
    ]);
    if rigs::saved(&st.userdata).is_empty() {
        let presets = board::presets();
        let p = |n: &str| {
            presets
                .iter()
                .find(|(x, _)| x.eq_ignore_ascii_case(n))
                .map(|x| x.1.clone())
                .unwrap_or_default()
        };
        let mk = |name: &str, tags: &[&str], color: usize, slot: u8, based: &str, board: Board| Rig {
            name: name.into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            color,
            slot,
            based_on: based.into(),
            board,
            scenes: Default::default(),
            saved: now_secs() - 86_400.0 * slot as f64,
        };
        let mut scenes: [Option<Board>; SCENES] = Default::default();
        let mut clean = lead.clone();
        for x in clean.blocks.iter_mut() {
            if x.def().id == "green" || x.def().id == "analog_delay" {
                x.on = false;
            }
        }
        scenes[0] = Some(clean);
        scenes[2] = Some(lead.clone());
        let mut warm = mk(
            "Reptilia lead, warmer",
            &["Indie", "Lead"],
            2,
            2,
            "Reptilia \u{b7} Lead",
            lead.clone(),
        );
        warm.scenes = scenes;
        for r in [
            mk("Glass clean", &["Pop", "Clean"], 0, 1, "Clean", p("Clean")),
            warm,
            mk("Bletchley lead", &["Rock", "Lead"], 3, 3, "Lead", p("Lead")),
            mk("Shimmer pad", &["Ambient"], 4, 4, "Ambient", p("Ambient")),
            mk(
                "Strokes rhythm, tight",
                &["Indie", "Crunch"],
                1,
                0,
                "Reptilia \u{b7} Rhythm",
                p("Crunch"),
            ),
        ] {
            rigs::store(&mut st.userdata, &r);
        }
    }
    // the room on the warmer lead, scene 3, with its Drive turned up a little since it was saved
    let mut now = lead.renewed();
    if let Some(g) = now.blocks.iter_mut().find(|x| x.def().id == "green") {
        g.set("drive", 4.6);
    }
    pb::set_my_board(&mut st.config, &now);
    let saved = rigs::saved(&st.userdata);
    let warm = saved.iter().find(|r| r.name == "Reptilia lead, warmer");
    RoomState {
        rig: warm.map(|r| r.name.clone()),
        based_on: warm.map(|r| r.based_on.clone()).unwrap_or_default(),
        scene: 2,
        scenes: warm.map(|r| r.scenes.clone()).unwrap_or_default(),
        names: ["Verse".into(), "Chorus".into(), "Solo".into(), String::new()],
    }
    .save(&mut st.config);
    if crate::gearindex::version() == 0 {
        let presets = board::presets();
        let mut tones = Vec::new();
        let rows: Vec<usize> = (0..st.groups.len())
            .filter(|&g| st.song(g).playable())
            .take(8)
            .collect();
        for (i, g) in rows.into_iter().enumerate() {
            let e = st.song(g);
            let (pn, pbrd) = &presets[i % presets.len()];
            let part = e
                .arrangements
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "Lead".into());
            tones.push((
                e.display_title(),
                e.artist.clone(),
                part,
                pn.to_lowercase(),
                pbrd.clone(),
            ));
            if i % 3 == 0 {
                tones.push((
                    e.display_title(),
                    e.artist.clone(),
                    "Rhythm".into(),
                    "crunch".into(),
                    presets[1].1.clone(),
                ));
            }
        }
        crate::gearindex::stage(tones);
    }
}

impl RoomView {
    /// Screenshot mode: the room in a state ("rig", "sounds", "save", "editor"), the guitar heard at a good
    /// level playing A2.
    pub fn stage_shot(&mut self, st: &mut AppState, what: &str) {
        self.shot = true;
        self.tab = Tab::Rig;
        self.listen.stage(-16.0, 45.04, false);
        self.save = None;
        self.typing = None;
        self.rack.board.editing = false;
        // (each shot starts on the guitar's room, the drawer on your sounds)
        self.rig_ui = rig::RigUi::default();
        // (no shot's A/B or scenes leak into the next)
        self.ab = false;
        self.rack.board.audition = None;
        self.state.names = Default::default();
        self.state.scenes = Default::default();
        self.state.scene = 0;
        self.cat = None;
        self.info = None;
        if let Some((id, _)) = crate::calibrate::current_input(st) {
            let mut cal = crate::calibrate::load(&st.config, &id);
            // (the data's own instrument, P37.6a: a bass player's data shows a bass room; "bass" on a guitar's)
            let own = crate::calibrate::default_instrument(&st.config);
            cal.instrument = if what == "bass" && !own.is_bass() {
                qrs_analysis::instrument::Instrument::Bass
            } else {
                own
            };
            crate::calibrate::save(&mut st.config, &id, &cal);
        }
        let b = self.rack.board_now(st);
        self.rack.board.sel = b
            .blocks
            .iter()
            .find(|x| x.def().id == if what == "editor" { "amp_crunch" } else { "green" })
            .map(|x| x.uid);
        match what {
            "sounds" => {
                self.go(st, Tab::Sounds);
                let saved = self.saved.clone();
                self.sounds.build(&saved, st);
                sounds::select(self, st, 0);
            }
            "save" => {
                self.go(st, Tab::Rig);
                self.open_save(st);
            }
            "editor" => {
                self.go(st, Tab::Rig);
                self.rack.board.editing = true;
            }
            "silver" | "silver_empty" => {
                self.go(st, Tab::Rig);
                let newest = |k| {
                    crate::captures::with(|l| {
                        if l.items.is_empty() {
                            *l = crate::captures::Library::open(&st.data.root);
                        }
                        l.of(k).max_by(|a, c| a.added.total_cmp(&c.added)).map(|i| i.id)
                    })
                    .unwrap_or(0)
                };
                let empty = what == "silver_empty";
                let b = |id: &str| board::Block::of(id).expect("catalogue model");
                let mut amp = b("amp_nam");
                let mut cab = b("cab_ir");
                if !empty {
                    amp.set_asset(newest(crate::captures::Kind::Nam));
                    cab.set_asset(newest(crate::captures::Kind::Ir));
                }
                let rig = Board::new(vec![
                    b("gate"),
                    b("green"),
                    amp,
                    cab,
                    b("analog_delay"),
                    b("spring"),
                ])
                .renewed();
                let uid = rig.blocks[2].uid;
                self.rack.board_edit(st, |x| *x = rig);
                self.rack.board_commit(st);
                self.rack.board.sel = Some(uid);
            }
            "gear" | "bass" => {
                self.go(st, Tab::Rig);
                if what == "bass"
                    && let Some((_, b)) = board::presets()
                        .into_iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case("bass"))
                {
                    let b = b.renewed();
                    let sel = b.first(Kind::Amp).map(|i| b.blocks[i].uid);
                    self.rack.board_edit(st, |x| *x = b);
                    self.rack.board_commit(st);
                    self.rack.board.sel = sel;
                }
                self.rig_ui.seg = rig::Seg::Gear;
                self.shelf_kind = Kind::Amp;
                self.shelf_page = 0;
            }
            "modeller" => {
                self.go(st, Tab::Rig);
                if let Some((id, _)) = crate::calibrate::current_input(st) {
                    let mut cal = crate::calibrate::load(&st.config, &id);
                    cal.kind = crate::calibrate::Kind::Modeller;
                    crate::calibrate::save(&mut st.config, &id, &cal);
                }
            }
            // P37.13: a rig of this model (a gate and it, it open); a capture with the newest file of its kind
            w if w.starts_with("unit:") || w == "cab" || w == "gate" || w == "empty" => {
                self.go(st, Tab::Rig);
                let b = |id: &str| board::Block::of(id).expect("catalogue model");
                let blocks = match w {
                    "empty" => vec![],
                    "cab" => vec![b("green"), b("amp_crunch"), b("cab_4x12")],
                    "gate" => vec![b("gate"), b("green"), b("amp_crunch"), b("cab_2x12")],
                    _ => {
                        let id = &w[5..];
                        let mut m = b(id);
                        if m.def().file {
                            let kind = if m.def().kind == Kind::Cab {
                                crate::captures::Kind::Ir
                            } else {
                                crate::captures::Kind::Nam
                            };
                            let newest = crate::captures::with(|l| {
                                if l.items.is_empty() {
                                    *l = crate::captures::Library::open(&st.data.root);
                                }
                                l.of(kind)
                                    .max_by(|a, c| a.added.total_cmp(&c.added))
                                    .map(|i| i.id)
                            });
                            if let Some(n) = newest {
                                m.set_asset(n);
                            }
                        }
                        let mut v = vec![b("green"), m];
                        if m.def().kind == Kind::Amp {
                            v.push(b("cab_2x12"));
                        }
                        v
                    }
                };
                let rig = Board::new(blocks).renewed();
                let sel = match w {
                    "empty" => None,
                    "cab" => rig.blocks.get(2).map(|x| x.uid),
                    "gate" => rig.blocks.get(2).map(|x| x.uid),
                    _ => rig.blocks.get(1).map(|x| x.uid),
                };
                self.rack.board_edit(st, |x| *x = rig);
                self.rack.board_commit(st);
                self.rack.board.sel = sel;
            }
            "scenes" | "ab" => {
                self.go(st, Tab::Rig);
                self.state.names = [
                    "Verse".into(),
                    "Big chorus lead".into(),
                    String::new(),
                    String::new(),
                ];
                let now = self.rack.board_now(st);
                self.state.scenes = [Some(now.clone()), Some(now.clone()), Some(now), None];
                self.state.scene = 1;
                if what == "ab" {
                    self.ab = true;
                }
            }
            // the Catalogue: everything, the amps, a search; the info card for a model and for a capture
            w if w.starts_with("catalogue") || w.starts_with("info") => {
                self.go(st, Tab::Rig);
                if w.starts_with("catalogue") {
                    let mut c = catalogue::Cat::new(false);
                    match w {
                        "catalogue_amps" => c.group = Some(0),
                        "catalogue_search" => c.query = "fuzz".into(),
                        "catalogue_captures" => c.mode = rig::ShelfMode::Captures,
                        _ => {}
                    }
                    self.cat = Some(c);
                } else if w == "info_capture" {
                    let id = crate::captures::with(|l| {
                        if l.items.is_empty() {
                            *l = crate::captures::Library::open(&st.data.root);
                        }
                        l.items
                            .iter()
                            .find(|i| i.skin.is_some())
                            .or(l.items.first())
                            .map(|i| i.id)
                    });
                    self.info = id.map(stage::Entry::Capture);
                } else {
                    self.info = board::catalog::find("amp_crunch").map(stage::Entry::Model);
                }
            }
            _ => self.go(st, Tab::Rig),
        }
        // (QUICKRS_BOARD_CHAIN / QUICKRS_BOARD_SEL: a board of these models on the stage, one selected, as the player's)
        if std::env::var("QUICKRS_BOARD_CHAIN").is_ok() {
            let sel = std::env::var("QUICKRS_BOARD_SEL").ok();
            self.rack.board_shot(st, None, sel.as_deref());
            self.rack.board.editing = false;
        }
        // recording (P23.9): "rec", "rec_paused", "rec_saved"
        if let Some(w) = what.strip_prefix("rec") {
            let dir = st.config.recordings_dir(&st.data.root);
            let w = match w.trim_start_matches('_') {
                "" => "recording",
                x => x,
            };
            st.rec
                .stage_for(crate::recording::Page::Room, &dir, w, &self.title());
        }
    }
}

fn push_text(s: &mut String, add: &str, max: usize) {
    for ch in add.chars() {
        if s.chars().count() < max && !ch.is_control() {
            s.push(ch);
        }
    }
}

pub fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
