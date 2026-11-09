//! Practice scenarios (new in quickRS, P37.11, owner request; D119-D121): the real player on the null audio devices
//! with a synthetic part, driven as a user drives it (keys through `handle`, clicks as the acts they send, the
//! tab's Ctrl-drag as its `TabAct`), the clock stepped by hand, and the state checked after each step. Each test is
//! one combination of the practice features (the loop / selection range, speed, the review, takes, note by note,
//! ear mode, continuous, the count-in, seeking, restart, the sections menu) and what the rules (rules.rs) say it
//! must do.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Event, Key, Modifiers};
use qrs_analysis::Judgement;
use qrs_audio::backend::{Backend, BufferSize, DeviceId};
use qrs_audio::decode::SongBuffer;
use qrs_audio::synth::SynthState;
use qrs_audio::{Engine, InputRoute, Route};
use qrs_formats::model::{Beat, Section};
use qrs_library::SongEntry;
use qrs_library::scan::PartEntry;

use super::tab::TabAct;
use super::{LOOP_LEAD_IN, PlayerView};
use crate::state::AppState;
use crate::transport::{Source, Transport};

/// The part: 16 bars of 4/4 at a beat a second, a note on every beat, four sections of four bars.
const BARS: usize = 16;
const SECTIONS: [(&str, f64, f64); 4] = [
    ("intro", 0.0, 16.0),
    ("verse", 16.0, 32.0),
    ("chorus", 32.0, 48.0),
    ("outro", 48.0, 64.0),
];

fn part() -> qrs_formats::Arrangement {
    let mut a = qrs_formats::Arrangement::new("Lead");
    a.song_length = BARS as f64 * 4.0 + 2.0;
    a.beats = (0..=BARS * 4)
        .map(|i| Beat {
            time: i as f64,
            measure: if i % 4 == 0 { (i / 4) as i32 + 1 } else { -1 },
        })
        .collect();
    // a riff that repeats every two bars, so the review finds its parts
    for i in 0..BARS * 4 {
        let fret = [3, 5, 7, 5, 3, 5, 8, 7][i % 8];
        a.notes
            .push(qrs_formats::Note::new(i as f64 + 0.25, (i % 3) as i32, fret));
    }
    a.sections = SECTIONS
        .iter()
        .enumerate()
        .map(|(k, (name, s, e))| Section {
            name: name.to_string(),
            number: k as i32 + 1,
            start: *s,
            end: *e,
        })
        .collect();
    a.phrases = SECTIONS.iter().map(|s| s.1).collect();
    a
}

fn null(id: &str) -> DeviceId {
    DeviceId {
        backend: Backend::Null,
        id: id.into(),
    }
}

static N: AtomicUsize = AtomicUsize::new(0);

/// The player on a scratch data folder and the null devices (a silent guitar input), its clock held.
pub(super) struct H {
    pub st: AppState,
    pub p: PlayerView,
    dir: std::path::PathBuf,
    /// The count-in's end as last seen, and when (`pin`): the count-in runs down on the stepped clock only, never
    /// the wall clock, so a test held up by a busy machine doesn't see it end early.
    pinned: Option<(Instant, Instant)>,
}

impl Drop for H {
    fn drop(&mut self) {
        if let Some(e) = self.p.engine() {
            e.pause();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl H {
    pub fn new() -> H {
        H::with(|_| {})
    }

    /// A player with the config set up by `cfg` first (as the owner left it).
    pub fn with(cfg: impl FnOnce(&mut qrs_library::Config)) -> H {
        let dir = std::env::temp_dir().join(format!(
            "qrs-scenario-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let entry = SongEntry {
            path: dir.join("synthetic_p.psarc").to_string_lossy().into_owned(),
            title: "Synthetic".into(),
            artist: "Tests".into(),
            length: BARS as f64 * 4.0 + 2.0,
            arrangements: vec![PartEntry {
                key: "lead".into(),
                name: "Lead".into(),
                tuning: "E Standard".into(),
                bonus: false,
                level: None,
            }],
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut st = AppState::for_test(&ctx, dir.clone(), entry.clone());
        cfg(&mut st.config);
        let loaded = crate::loading::Loaded {
            g: 0,
            entry,
            part: "lead".into(),
            arr: part(),
            lyrics: Vec::new(),
            song: None,
            stems: None,
            audio_error: String::new(),
            key: None,
        };
        let mut p = PlayerView::new(&mut st, loaded);
        // the engine on the null devices, a silent guitar plugged in; the clock held (stepped by `tick`)
        let e = Arc::new(Engine::new());
        e.open(Route {
            input: Some(InputRoute {
                device: null("null"),
                channel: 0,
            }),
            output: Some(null("null")),
            exclusive: false,
            buffer: BufferSize::Frames(256),
            base_channel: 0,
        });
        assert!(e.settle(Duration::from_secs(10)));
        let song = SongBuffer::silent(e.rate(), BARS as f64 * 4.0 + 2.0);
        e.load_song(song.clone());
        let was_playing = p.transport.playing();
        p.transport = Transport::new(Source::Engine {
            e,
            song: Arc::new(song),
        });
        // held first, then played: the held clock keeps "playing" itself. (Read back from the engine, as hold() does,
        // it raced the null device's timer thread: Engine::play waits for it at most 50 ms, which a machine running
        // the other scenarios' engines in parallel sometimes misses, and the song then opened stopped.)
        p.transport.hold(Some(0.0));
        if was_playing {
            p.transport.play();
        }
        if p.countdown_until.is_some() {
            // (the song opens now: the count-in from here, not from before the engine came up)
            p.countdown_until = Some(Instant::now() + Duration::from_secs_f32(super::COUNTDOWN));
        }
        let mut h = H {
            st,
            p,
            dir,
            pinned: None,
        };
        h.pin();
        h
    }

    /// A key press, as the window sends it.
    pub fn key(&mut self, key: Key) {
        self.key_with(key, Modifiers::NONE);
    }

    pub fn key_with(&mut self, key: Key, modifiers: Modifiers) {
        let ev = Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        self.p.handle(&mut self.st, &ev);
        self.pin();
    }

    /// Note the count-in's end and the wall time now (after each thing the user does and each frame).
    fn pin(&mut self) {
        self.pinned = self.p.countdown_until.map(|u| (u, Instant::now()));
    }

    /// The count-in's end moved on to the wall clock now, `dt` stepped seconds nearer: what was left of it at the
    /// last `pin`, less `dt`.
    fn step_count_in(&mut self, dt: f64) {
        let Some(until) = self.p.countdown_until else {
            return;
        };
        let left = match self.pinned {
            Some((u, at)) if u == until => u.saturating_duration_since(at),
            _ => until.saturating_duration_since(Instant::now()),
        };
        let left = left.saturating_sub(Duration::from_secs_f64(dt));
        self.p.countdown_until = Some(Instant::now() + left);
    }

    pub fn ctrl(&mut self, key: Key) {
        self.key_with(key, Modifiers::COMMAND);
    }

    /// A click on a key that sends `name`.
    pub fn act(&mut self, name: &str) {
        self.p.act(&mut self.st, name);
        self.pin();
    }

    /// Ctrl-drag over the tab from chart time a to b (the tab snaps to bars; here they're given as bars).
    pub fn select(&mut self, a: f64, b: f64) {
        self.p.tab.sel = Some((a, b));
        self.p.tab_act(TabAct::Selected);
        self.pin();
    }

    /// `wall` seconds pass in steps of a frame: a count-in runs down, the song moves at its speed, the player
    /// updates.
    pub fn tick(&mut self, wall: f64) {
        let mut left = wall;
        while left > 1e-9 {
            let dt = left.min(1.0 / 30.0);
            left -= dt;
            self.step_count_in(dt);
            if self.p.pending_speed.is_some() {
                self.p.speed_at = self
                    .p
                    .speed_at
                    .checked_sub(Duration::from_secs_f64(dt))
                    .unwrap_or(self.p.speed_at);
            }
            self.p.transport.step_held(dt);
            self.p.update(&mut self.st);
            self.pin();
        }
    }

    /// Long enough for a count-in and a speed change to settle.
    pub fn settle(&mut self) {
        self.tick(3.2);
    }

    pub fn now(&self) -> f64 {
        self.p.now()
    }

    pub fn playing(&self) -> bool {
        self.p.transport.playing()
    }

    pub fn counting(&self) -> bool {
        self.p.countdown_until.is_some()
    }

    pub fn range(&self) -> Option<(f64, f64)> {
        self.p.loop_a.zip(self.p.loop_b)
    }

    pub fn speed(&self) -> f64 {
        self.p.speed()
    }

    /// The last toast said (its words), if any. (Not by whether it's still up: that's 1.6 s of wall clock, which a
    /// busy machine can run past between the action and the check.)
    pub fn toast(&self) -> String {
        self.p
            .toast
            .as_ref()
            .map(|t| format!("{}{}", t.0, t.1))
            .unwrap_or_default()
    }

    /// Every note before chart time `t` judged: hits, with misses in [miss_a, miss_b).
    pub fn played_to(&mut self, t: f64, miss: (f64, f64)) {
        for g in self.p.scorer.groups.iter_mut().filter(|g| g.t < t) {
            g.state = Some(if miss.0 <= g.t && g.t < miss.1 {
                Judgement::Miss
            } else {
                Judgement::Hit
            });
        }
    }

    /// A whole run played to the end (rough in the chorus), the song stopped there: its review saved.
    pub fn a_run(&mut self) {
        self.played_to(100.0, (34.0, 40.0));
        let d = self.p.transport.duration();
        self.p.transport.seek(d);
        self.p.transport.pause();
        self.p.countdown_until = None;
        self.tick(0.1);
        assert!(self.p.review.is_some(), "the run's review is made");
        // back on the song: the results card closed
        self.act("summary_close");
    }

    /// Play from where it is (Space), through any count-in.
    pub fn play(&mut self) {
        self.key(Key::Space);
        self.settle();
    }
}

/// The record each scenario keeps of how it went before the fix (P37.11 log): works / broken / illogical.
#[allow(dead_code)]
const _RECORD: () = ();

// -- the count-in (D121) ---------------------------------------------------------------------------------------

#[test]
fn countdown_on_counts_in_everywhere() {
    let mut h = H::new();
    assert!(h.counting(), "a song opens counting in");
    h.settle();
    assert!(h.playing());
    // a review spot from stopped
    h.a_run();
    h.key(Key::E);
    assert!(h.p.review_open());
    h.key(Key::Enter);
    assert!(h.range().is_some());
    assert!(h.counting(), "a review spot from stopped counts in");
    h.settle();
    assert!(h.playing());
    // continuous from stopped
    h.key(Key::Space);
    assert!(!h.playing());
    h.key(Key::O);
    assert!(h.counting(), "continuous from stopped counts in");
}

#[test]
fn countdown_off_never_counts_in() {
    let mut h = H::with(|c| c.set("countdown", false));
    assert!(!h.counting(), "the song starts without a count-in");
    assert!(h.playing(), "and plays at once");
    // a review spot from stopped
    h.a_run();
    h.key(Key::E);
    h.key(Key::Enter);
    assert!(!h.counting(), "a review spot: no count-in");
    assert!(h.playing());
    // continuous from stopped
    h.key(Key::Space);
    h.key(Key::O);
    assert!(!h.counting(), "continuous: no count-in");
    assert!(h.playing());
    // the review's whole song
    h.key(Key::Space);
    h.key(Key::E);
    h.act("review:whole");
    assert!(!h.counting(), "the whole song: no count-in");
    assert!(h.playing());
    // a seek while playing plays on; a loop going round doesn't count in
    h.act("fwd1");
    assert!(h.playing() && !h.counting());
    // a take
    h.key(Key::Space);
    h.key(Key::L);
    h.ctrl(Key::R);
    assert!(h.p.take.is_some(), "{}", h.toast());
    assert!(!h.counting(), "a take: no count-in");
    assert!(h.playing());
}

#[test]
fn opened_paused_with_the_countdown_off_waits_for_play() {
    let mut h = H::with(|c| c.set("countdown", false));
    h.p.open_paused();
    assert!(!h.playing(), "a double-click opens it paused");
    h.key(Key::Space);
    assert!(h.playing() && !h.counting(), "the first Play plays at once");
}

#[test]
fn a_ctrl_drag_while_playing_loops_the_bars_selected() {
    let mut h = H::new();
    h.settle();
    h.tick(2.0);
    h.select(40.0, 44.0);
    assert_eq!(h.range(), Some((40.0, 44.0)));
    assert!(
        (h.now() - (40.0 - LOOP_LEAD_IN)).abs() < 0.05,
        "to its lead-in: {}",
        h.now()
    );
    assert!(h.playing());
    h.tick(7.0);
    assert!(
        h.now() < 44.0 && h.now() >= 40.0 - LOOP_LEAD_IN - 0.01,
        "round it: {}",
        h.now()
    );
}

#[test]
fn a_hand_set_loop_waits_while_you_jump_sections() {
    let mut h = H::new();
    h.settle();
    h.p.transport.seek(4.0);
    h.key(Key::OpenBracket);
    h.p.transport.seek(8.0);
    h.key(Key::CloseBracket);
    h.key(Key::PageDown);
    assert_eq!(h.range(), Some((4.0, 8.0)), "a [ ] loop doesn't move");
    h.tick(1.5);
    assert!(
        h.now() > 15.0,
        "the song plays on in the next section: {}",
        h.now()
    );
}

#[test]
fn a_clean_pass_at_seventy_percent_counts_and_keeps_the_speed() {
    let mut h = H::new();
    h.settle();
    for _ in 0..6 {
        h.ctrl(Key::Equals);
    }
    h.settle();
    let speed = h.speed();
    h.p.transport.seek(20.0);
    h.key(Key::L);
    h.played_to(32.0, (0.0, 0.0));
    h.tick(18.0); // (12 song seconds at 70%)
    assert_eq!(h.p.loop_passes, 1, "{}", h.now());
    assert_eq!(h.p.loop_acc, Some(1.0));
    assert!((h.speed() - speed).abs() < 1e-9, "the speed stays");
    assert_eq!(h.range(), Some((16.0, 32.0)));
}

#[test]
fn the_review_stops_the_selection_playing() {
    let mut h = H::new();
    h.settle();
    h.a_run();
    h.select(16.0, 24.0);
    h.key(Key::P);
    assert_ne!(h.p.synth.state, SynthState::Idle);
    h.key(Key::E);
    assert!(h.p.review_open());
    assert_eq!(
        h.p.synth.state,
        SynthState::Idle,
        "nothing plays under the review"
    );
    h.key(Key::Escape);
    assert_eq!(h.range(), Some((16.0, 24.0)), "the selection's range as it was");
}

#[test]
fn the_countdown_key_switches_it_everywhere_and_is_kept() {
    let mut h = H::new();
    h.settle();
    h.act("countdown");
    assert!(!h.st.config.bool("countdown", true), "kept in the config");
    assert!(h.toast().contains("off"), "{}", h.toast());
    h.key(Key::Space);
    h.key(Key::L);
    h.key(Key::O);
    assert!(!h.counting());
    assert!(h.playing());
    h.act("countdown");
    assert!(h.st.config.bool("countdown", false));
    h.key(Key::O); // continuous off
    h.key(Key::Space); // stop
    h.key(Key::O); // and on again: counts in
    assert!(h.counting(), "on again: it counts in again");
}

#[test]
fn a_seek_during_the_count_in_counts_in_from_there() {
    let mut h = H::new();
    h.tick(0.5);
    assert!(h.counting());
    h.act("fwd1");
    assert!(h.counting(), "the count-in goes on from the new place");
    h.settle();
    assert!(h.playing());
}

// -- speed (D120) ----------------------------------------------------------------------------------------------

#[test]
fn speed_keys_work_while_the_review_is_up() {
    let mut h = H::new();
    h.settle();
    h.a_run();
    h.key(Key::E);
    assert!(h.p.review_open());
    h.ctrl(Key::Equals); // slower
    assert!(
        h.speed() < 0.999,
        "the speed changes under the review: {}",
        h.speed()
    );
    h.key(Key::Enter); // play the spot
    h.settle();
    assert!(h.speed() < 0.999, "the spot plays at the speed set");
    let s = h.speed();
    h.ctrl(Key::Equals);
    h.settle();
    assert!(h.speed() < s, "and slows down while it plays");
}

#[test]
fn the_selection_player_follows_the_speed() {
    let mut h = H::new();
    h.settle();
    h.key(Key::Space);
    h.select(16.0, 24.0);
    h.key(Key::P);
    assert_ne!(h.p.synth.state, SynthState::Idle, "P plays the selection");
    h.ctrl(Key::Equals);
    h.tick(0.6);
    let want = h.p.selection_speed();
    assert!(want < 0.999);
    assert!(
        (h.p.synth.speed - want).abs() < 1e-6,
        "restarted at the new speed: {} vs {want}",
        h.p.synth.speed
    );
}

// -- the practice range (D119) ---------------------------------------------------------------------------------

#[test]
fn a_selection_is_the_loop() {
    let mut h = H::new();
    h.settle();
    h.key(Key::L); // the intro looped
    assert_eq!(h.range(), Some((0.0, 16.0)));
    h.select(16.0, 24.0);
    assert_eq!(h.range(), Some((16.0, 24.0)), "the selection is the range now");
    // Space plays the song in it (the loop), not the synth
    h.key(Key::Space);
    h.key(Key::Space);
    h.settle();
    assert!(h.playing(), "Space plays the song");
    assert_eq!(h.p.synth.state, SynthState::Idle);
    h.tick(9.0);
    assert!(
        h.now() >= 16.0 - LOOP_LEAD_IN - 0.01 && h.now() < 24.0,
        "it loops: {}",
        h.now()
    );
}

#[test]
fn setting_a_loop_replaces_the_selection_and_clearing_clears_both() {
    let mut h = H::new();
    h.settle();
    h.select(16.0, 24.0);
    h.p.transport.seek(40.0);
    h.key(Key::L);
    assert_eq!(h.range(), Some((32.0, 48.0)));
    assert_eq!(h.p.tab.sel, None, "the selection gave way to the loop");
    h.select(4.0, 8.0);
    h.key(Key::Backslash);
    assert_eq!(h.range(), None);
    assert_eq!(h.p.tab.sel, None, "clearing the loop clears the selection");
    h.select(4.0, 8.0);
    h.key(Key::Escape);
    assert_eq!(h.range(), None, "Esc on a selection clears the range too");
}

#[test]
fn a_seek_past_the_loop_plays_on_and_jumping_sections_moves_a_section_loop() {
    let mut h = H::new();
    h.settle();
    h.p.transport.seek(20.0);
    h.key(Key::L); // the verse
    assert_eq!(h.range(), Some((16.0, 32.0)));
    // the sections menu: the chorus
    h.act("sections");
    h.p.go_to_section(2);
    assert_eq!(
        h.range(),
        Some((32.0, 48.0)),
        "a section loop follows a jump to another section"
    );
    h.tick(1.5);
    assert!(h.now() >= 31.0, "{}", h.now());
    // a hand-set loop, then a click after its end: the song plays on from there
    h.key(Key::Backslash);
    h.p.transport.seek(4.0);
    h.key(Key::OpenBracket);
    h.p.transport.seek(8.0);
    h.key(Key::CloseBracket);
    assert_eq!(h.range(), Some((4.0, 8.0)));
    h.p.tab_act(TabAct::Seek(30.0));
    h.tick(0.5);
    assert!(h.now() > 30.0, "played on past the loop: {}", h.now());
    assert_eq!(h.range(), Some((4.0, 8.0)), "the loop is kept for later");
}

#[test]
fn restart_while_looping_restarts_the_loop() {
    let mut h = H::new();
    h.settle();
    h.p.transport.seek(36.0);
    h.key(Key::L);
    h.tick(2.0);
    h.key(Key::Home);
    assert!(
        (h.now() - (32.0 - LOOP_LEAD_IN)).abs() < 0.05,
        "Home in a loop: its lead-in ({})",
        h.now()
    );
    assert_eq!(h.range(), Some((32.0, 48.0)));
    h.key(Key::Backslash);
    h.key(Key::Home);
    assert!(h.now() < 0.05, "without a loop, the song's start");
}

// -- the review (D120, D121) -----------------------------------------------------------------------------------

#[test]
fn opening_and_closing_the_review_returns_to_where_you_were() {
    let mut h = H::new();
    h.settle();
    h.a_run();
    h.p.transport.seek(20.0);
    h.key(Key::L);
    h.play();
    let at = h.now();
    h.key(Key::E);
    assert!(h.p.review_open());
    assert!(!h.playing(), "the review pauses");
    h.key(Key::E);
    assert!(!h.p.review_open());
    assert_eq!(h.range(), Some((16.0, 32.0)), "the loop as it was");
    assert!((h.now() - at).abs() < 0.05, "the same place");
    h.settle();
    assert!(h.playing(), "and playing again, as it was");
}

#[test]
fn leaving_a_review_spot_gives_back_your_loop() {
    let mut h = H::new();
    h.settle();
    h.a_run();
    h.p.transport.seek(20.0);
    h.key(Key::L);
    h.key(Key::E);
    h.key(Key::Enter);
    let spot = h.range();
    assert!(spot.is_some() && spot != Some((16.0, 32.0)), "{spot:?}");
    h.act("review:leave");
    assert_eq!(h.range(), Some((16.0, 32.0)), "your loop back");
}

#[test]
fn the_review_during_a_take_says_so() {
    let mut h = H::new();
    h.settle();
    h.a_run();
    h.key(Key::L);
    h.ctrl(Key::R);
    assert!(h.p.take.as_ref().is_some_and(|s| s.going()), "{}", h.toast());
    h.key(Key::E);
    assert!(!h.p.review_open(), "not over a take");
    assert!(h.toast().contains("waits for the take"), "{}", h.toast());
}

#[test]
fn the_review_while_note_by_note_waits() {
    let mut h = H::new();
    h.settle();
    h.a_run();
    h.p.transport.seek(0.0);
    h.p.scorer.reset_from(0.0);
    h.key(Key::W);
    assert!(h.p.nbn);
    h.play();
    h.tick(0.5);
    assert!(h.p.waiting.is_some(), "waiting at the first note");
    h.key(Key::E);
    assert!(h.p.review_open());
    assert!(h.p.waiting.is_none());
    h.key(Key::E);
    assert!(h.p.nbn, "note by note still on");
}

// -- takes -----------------------------------------------------------------------------------------------------

#[test]
fn a_take_in_a_loop_at_seventy_percent() {
    let mut h = H::new();
    h.settle();
    h.p.transport.seek(20.0);
    h.key(Key::L);
    for _ in 0..6 {
        h.ctrl(Key::Equals);
    }
    h.settle();
    let speed = h.speed();
    assert!((speed - 0.7).abs() < 0.051, "{speed}");
    h.ctrl(Key::R);
    let s = h.p.take.as_ref().expect("a take");
    assert!((s.speed - speed).abs() < 1e-9);
    assert_eq!((s.a, s.b), (16.0, 32.0));
    assert!(h.counting());
    h.settle();
    assert!(h.playing());
    assert_eq!(h.range(), Some((16.0, 32.0)));
}

#[test]
fn a_take_with_note_by_note_on_says_it_stands_aside() {
    let mut h = H::new();
    h.settle();
    h.key(Key::W);
    h.key(Key::L);
    h.ctrl(Key::R);
    assert!(h.p.take.is_some());
    assert!(h.toast().contains("note by note"), "{}", h.toast());
}

#[test]
fn listening_back_then_seeking_stops_listening() {
    let mut h = H::new();
    h.settle();
    h.p.transport.seek(30.0);
    h.p.stage_take("take");
    h.p.transport.pause();
    h.key(Key::B);
    assert!(h.p.take.as_ref().is_some_and(|s| s.listening()));
    h.act("fwd1");
    assert!(
        !h.p.take.as_ref().is_some_and(|s| s.listening()),
        "a seek stops listening back"
    );
}

#[test]
fn listening_back_follows_the_speed() {
    let mut h = H::new();
    h.settle();
    h.p.transport.seek(30.0);
    h.p.stage_take("take");
    h.p.transport.pause();
    h.ctrl(Key::Equals);
    h.tick(0.6);
    h.key(Key::B);
    let l = h.p.take.as_ref().and_then(|s| s.listen_speed());
    assert!(
        l.is_some_and(|v| (v - h.speed()).abs() < 1e-9),
        "{l:?} vs {}",
        h.speed()
    );
}

// -- modes -----------------------------------------------------------------------------------------------------

#[test]
fn ear_mode_and_a_loop() {
    let mut h = H::new();
    h.settle();
    h.key(Key::H);
    h.key(Key::L);
    h.played_to(10.0, (0.0, 0.0));
    h.tick(0.1);
    let shown = h.p.revealed.iter().filter(|r| **r).count();
    assert!(shown > 0);
    h.tick(17.0); // round the loop
    assert!(h.p.ear);
    assert!(
        h.p.revealed.iter().filter(|r| **r).count() >= shown,
        "found notes stay found"
    );
}

#[test]
fn continuous_needs_a_range_and_goes_with_it() {
    let mut h = H::new();
    h.settle();
    h.key(Key::O);
    assert!(!h.p.continuous);
    h.key(Key::L);
    h.key(Key::O);
    assert!(h.p.continuous);
    h.key(Key::Backslash);
    assert!(!h.p.continuous, "no range, no continuous");
    h.select(16.0, 24.0);
    h.key(Key::O);
    assert!(h.p.continuous, "a selection is a range for continuous too");
}

/// P37.6b (D135): the tab's toolbar has a row of its own: drawn at 1100 x 700 and 1440 x 900 (and in full-screen
/// tab), it never reaches into the tab's rows, the first row included.
#[test]
fn the_tab_toolbar_is_never_over_the_tab() {
    let mut h = H::new();
    qrs_ui::text::install(&h.st.ctx);
    h.settle();
    for full in [false, true] {
        h.p.tab_full = full;
        for (w, ht) in [(1100.0, 700.0), (1440.0, 900.0)] {
            let ctx = h.st.ctx.clone();
            let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, ht));
            for frame in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(area),
                    time: Some(frame as f64 * 0.05),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| h.p.ui(ui, &mut h.st, area));
                out.textures_delta.clear();
            }
            let (tools, tab) = (h.p.tools_rect, h.p.tab.view_rect());
            assert!(
                tools.width() > 0.0 && tab.height() > 0.0,
                "{w}x{ht}: {tools:?} {tab:?}"
            );
            assert!(
                tools.max.y <= tab.min.y,
                "{w}x{ht} full {full}: the toolbar {tools:?} over the tab {tab:?}"
            );
            assert!(
                tools.min.x >= area.min.x && tools.max.x <= area.max.x,
                "{tools:?} in {area:?}"
            );
        }
    }
}

/// P47.1 (owner): the RS Highway's full screen is the whole window, edge to edge (the tab's keeps its margin and the
/// scrollbar's groove); the score tile floats at its top right, and folds to a strip (config `hw_full_score`).
#[test]
fn the_highway_full_screen_is_edge_to_edge_with_the_score() {
    let mut h = H::new();
    qrs_ui::text::install(&h.st.ctx);
    h.settle();
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
    let frame = |h: &mut H| {
        let ctx = h.st.ctx.clone();
        for k in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(area),
                time: Some(k as f64 * 0.05),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| h.p.ui(ui, &mut h.st, area));
            out.textures_delta.clear();
        }
    };
    h.p.tab_full = true;
    h.p.hw_on = true;
    frame(&mut h);
    assert_eq!(h.p.well, area, "the highway fills the window");
    let tile = h.p.hw_score_rect;
    assert!(tile.width() > 200.0 && tile.height() > 60.0, "{tile:?}");
    assert!(
        area.max.x - tile.max.x < 40.0 && tile.min.y - area.min.y < 80.0,
        "top right, under the toolbar: {tile:?}"
    );
    h.st.config.set("hw_full_score", false);
    frame(&mut h);
    assert_eq!(h.p.hw_score_rect, egui::Rect::NOTHING, "folded to its strip");
    // the tab's full screen keeps its margin and the scrollbar's groove, and has no score tile
    h.st.config.set("hw_full_score", true);
    h.p.hw_on = false;
    frame(&mut h);
    assert!(h.p.well.min.x > area.min.x && h.p.well.max.x < area.max.x - 20.0);
}

/// P48.1 (owner): the tuner opens on the song's own tuning (no list of tunings), each string can be set to any note,
/// and it works out guitar or bass by itself.
#[test]
fn the_tuner_is_the_songs_and_each_string_can_be_set() {
    use qrs_analysis::instrument::Instrument;
    let mut h = H::new();
    let (name, song) = h.p.tuner_targets();
    assert_eq!(song, vec![40, 45, 50, 55, 59, 64], "a guitar part: six strings");
    assert!(name.contains("this song"), "{name}");
    assert_eq!(h.p.tuner_inst(), Instrument::Guitar);
    // the low string a whole step down by hand: Drop D, named, and the retune card stands aside
    let mut t = song.clone();
    t[0] -= 2;
    h.p.tuner_custom = Some(t.clone());
    assert_eq!(h.p.tuner_targets(), ("Drop D (yours)".to_string(), t));
    h.p.tuner_custom = Some(vec![38, 45, 50, 55, 57, 62]);
    assert_eq!(h.p.tuner_targets().0, "Your tuning");
    h.p.tuner_custom = None;
    // a bass part with the settings on guitar: a bass tuner (four strings, the bass detector)
    h.p.chart.arr.is_bass = true;
    h.p.chart.opens = vec![28, 33, 38, 43];
    assert_eq!(h.p.tuner_inst(), Instrument::Bass);
    // a 5-string bass set in the settings: five strings
    h.p.instrument = Instrument::Bass5;
    assert_eq!(h.p.tuner_inst(), Instrument::Bass5);
    assert_eq!(h.p.tuner_targets().1.len(), 5);
}
