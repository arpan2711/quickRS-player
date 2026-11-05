//! The guided guitar setup (P23.8, owner request; its calibration reworked in P23.10; new code, no Python source): the steps Settings > Audio walks a
//! new user through (the wizard is drawn by `settings/guide.rs` on the Audio page's own pieces, and inside the
//! first-time setup's guitar page, P23.14), what the level check says about the signal and which open strings the
//! note check has heard.
//!
//! config.json `guitar_setup`: "done" once the setup was finished ("shown" / "dismissed" from the fading first-run
//! card it had before the first-time setup replaced it).

use std::collections::VecDeque;

use qrs_library::Config;
use serde_json::Value;

/// The config.json key recording the prompt and the setup.
pub const KEY: &str = "guitar_setup";

/// The steps, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Input,
    Output,
    /// What's plugged in, and whether it's a guitar or a bass (before Level: its advice depends on both, P36.6).
    Kind,
    Level,
    Calibrate,
    /// What calibration recommends, as sliders to adjust while you play (P23.10).
    Settings,
    Notes,
    /// The timing tool (P36.2): guitar timing and screen timing.
    Timing,
    Done,
}

impl Step {
    pub const ALL: [Step; 9] = [
        Step::Input,
        Step::Output,
        Step::Kind,
        Step::Level,
        Step::Calibrate,
        Step::Settings,
        Step::Notes,
        Step::Timing,
        Step::Done,
    ];

    pub fn index(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn key(self) -> &'static str {
        match self {
            Step::Input => "input",
            Step::Output => "output",
            Step::Level => "level",
            Step::Kind => "kind",
            Step::Calibrate => "calibrate",
            Step::Settings => "settings",
            Step::Notes => "notes",
            Step::Timing => "timing",
            Step::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Step> {
        Step::ALL.into_iter().find(|st| st.key() == s)
    }

    /// The step's heading.
    pub fn title(self) -> &'static str {
        match self {
            Step::Input => "Pick your guitar's input",
            Step::Output => "Pick where you hear it",
            Step::Level => "Is it working?",
            Step::Kind => "What's plugged in?",
            Step::Calibrate => "Calibrate",
            Step::Settings => "Your input settings",
            Step::Notes => "Check the open strings",
            // (the play-along, P38.4: its Advanced is the click tool, titled on its own in the guide)
            Step::Timing => "Can you play this?",
            Step::Done => "All set",
        }
    }

    /// Its short name (the summary's rows, the dots' tooltips).
    pub fn short(self) -> &'static str {
        match self {
            Step::Input => "Input",
            Step::Output => "Output",
            Step::Level => "Level",
            Step::Kind => "Kind of input",
            Step::Calibrate => "Calibration",
            Step::Settings => "Settings",
            Step::Notes => "Notes",
            Step::Timing => "Timing",
            Step::Done => "Done",
        }
    }
}

/// How a step was left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mark {
    #[default]
    Todo,
    Done,
    Skipped,
}

/// Where the setup is, and how each step was left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
    pub step: Step,
    pub marks: [Mark; 9],
}

impl Default for Flow {
    fn default() -> Self {
        Flow {
            step: Step::Input,
            marks: [Mark::Todo; 9],
        }
    }
}

impl Flow {
    pub fn at(step: Step) -> Flow {
        Flow {
            step,
            ..Flow::default()
        }
    }

    pub fn mark(&self, s: Step) -> Mark {
        self.marks[s.index()]
    }

    /// Next: this step is done, on to the next one (it stays on the last).
    pub fn next(&mut self) {
        self.leave(Mark::Done);
    }

    /// Skip: on to the next step, this one marked skipped (unless it was done before).
    pub fn skip(&mut self) {
        self.leave(Mark::Skipped);
    }

    fn leave(&mut self, how: Mark) {
        let i = self.step.index();
        if self.step == Step::Done {
            return;
        }
        if !(how == Mark::Skipped && self.marks[i] == Mark::Done) {
            self.marks[i] = how;
        }
        self.step = Step::ALL[i + 1];
    }

    /// Back a step (none before the first).
    pub fn back(&mut self) -> bool {
        let i = self.step.index();
        if i == 0 {
            return false;
        }
        self.step = Step::ALL[i - 1];
        true
    }
}

// -- the level check ---------------------------------------------------------------------------------------------

/// Above this peak (dBFS) something is coming in.
pub const HEARD_DB: f32 = -50.0;
/// Below this peak a hard strum is too quiet for the trim to bring up to the boards' level (-12 dBFS, +24 dB at most).
pub const QUIET_DB: f32 = -36.0;
/// Over this peak the interface is near full scale.
pub const HOT_DB: f32 = -4.0;
/// Over this, clipping.
pub const CLIP_DB: f32 = -1.0;
/// How long the check looks back (s), and how long it waits for a first note before it says nothing's coming in.
const WINDOW_S: f64 = 4.0;
const WAIT_S: f64 = 5.0;

/// What the level check says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// No input is listening.
    NoInput,
    /// Listening, nothing played yet.
    Waiting,
    /// Nothing coming in for a while.
    Silent,
    Quiet,
    Good,
    Hot,
    Clipping,
}

impl Level {
    /// (the headline, what to do) in plain words.
    pub fn words(self) -> (&'static str, &'static str) {
        match self {
            Level::NoInput => (
                "No input is listening",
                "Go back a step and pick the input your guitar is plugged into.",
            ),
            Level::Waiting => (
                "Play your guitar",
                "Strum a few chords, as hard as you normally play.",
            ),
            Level::Silent => (
                "Nothing is coming in",
                "Check the guitar's volume knob is up, the input channel (step 1), and on an interface: the \
                 instrument (INST or Hi-Z) switch on for that input, and its gain knob turned up.",
            ),
            Level::Quiet => (
                "Very quiet",
                "Turn the gain knob on your interface up (or the guitar's volume). Calibrate can add 24 dB, no more.",
            ),
            Level::Good => (
                "Good level",
                "A healthy signal. Calibrate fine-tunes it in a moment.",
            ),
            Level::Hot => (
                "A little hot",
                "Close to the top: a touch less gain on your interface is safer.",
            ),
            Level::Clipping => (
                "Too loud: it's clipping",
                "Turn the gain knob on your interface down until its light stays off on your hardest strum.",
            ),
        }
    }

    pub fn ok(self) -> bool {
        matches!(self, Level::Good | Level::Hot)
    }

    /// `words`, for the kind of input (P36.6): an amp modeller has a USB level, not a gain knob; a microphone has
    /// a distance and its own gain.
    pub fn words_for(self, kind: crate::calibrate::Kind) -> (&'static str, &'static str) {
        use crate::calibrate::Kind;
        match (self, kind) {
            (Level::Silent, Kind::Modeller) => (
                "Nothing is coming in",
                "Check the modeller is on and its USB audio is the input picked in step 1 (and its channel: often \
                 2 on a modeller), and that its master volume and USB level aren't at zero.",
            ),
            (Level::Quiet, Kind::Modeller) => (
                "Very quiet",
                "Raise the modeller's USB (record) level in its app or menu, or its patch's level. Calibrate can add \
                 24 dB, no more.",
            ),
            (Level::Hot | Level::Clipping, Kind::Modeller) => (
                if self == Level::Hot {
                    "A little hot"
                } else {
                    "Very hot: at the top of the scale"
                },
                "A modeller often sends at full scale: Calibrate turns it down to fit. Lower its USB (record) level \
                 if it has one.",
            ),
            (Level::Quiet, Kind::Mic) => (
                "Very quiet",
                "Move the microphone closer (20-30 cm from the 12th fret) or turn its gain up.",
            ),
            (Level::Hot | Level::Clipping, Kind::Mic) => (
                if self == Level::Hot {
                    "A little hot"
                } else {
                    "Too loud: it's clipping"
                },
                "Move the microphone a little further away, or turn its gain down.",
            ),
            _ => self.words(),
        }
    }
}

/// The level check's memory: the peaks of the last few seconds (fed once a frame), when sound was first heard and
/// when the interface last clipped.
#[derive(Clone, Debug, Default)]
pub struct LevelWatch {
    start: Option<f64>,
    recent: VecDeque<(f64, f32)>,
    heard: bool,
    clips: Option<u32>,
    clip_at: Option<f64>,
}

fn db(v: f32) -> f32 {
    20.0 * v.max(1e-7).log10()
}

impl LevelWatch {
    /// One frame: the time (s, any clock), the input's peak (0..1, before the trim) and its clip count.
    pub fn feed(&mut self, t: f64, peak: f32, clips: u32) {
        self.start.get_or_insert(t);
        self.recent.push_back((t, db(peak)));
        while self.recent.front().is_some_and(|(t0, _)| t - t0 > WINDOW_S) {
            self.recent.pop_front();
        }
        if db(peak) > HEARD_DB {
            self.heard = true;
        }
        if self.clips.is_some_and(|c| clips > c) {
            self.clip_at = Some(t);
        }
        self.clips = Some(clips);
    }

    /// The highest peak of the last few seconds (dBFS).
    pub fn peak_db(&self) -> f32 {
        self.recent.iter().map(|r| r.1).fold(-140.0, f32::max)
    }

    /// What it says at time `t` (`listening`: an input is open).
    pub fn verdict(&self, t: f64, listening: bool) -> Level {
        if !listening {
            return Level::NoInput;
        }
        let pk = self.peak_db();
        if pk > CLIP_DB || self.clip_at.is_some_and(|c| t - c < WINDOW_S) {
            Level::Clipping
        } else if pk <= HEARD_DB {
            let waited = self.start.map(|s| t - s).unwrap_or(0.0);
            if !self.heard && waited > WAIT_S {
                Level::Silent
            } else {
                Level::Waiting
            }
        } else if pk < QUIET_DB {
            Level::Quiet
        } else if pk > HOT_DB {
            Level::Hot
        } else {
            Level::Good
        }
    }
}

// -- the note check ----------------------------------------------------------------------------------------------

/// The open strings in E standard (MIDI), low to high, and their names.
pub const OPEN: [i32; 6] = [40, 45, 50, 55, 59, 64];
pub const OPEN_NAMES: [&str; 6] = ["E", "A", "D", "G", "B", "e"];

/// A bass's open strings (P36.6): E1 A1 D2 G2, and a 5-string's low B0 first.
pub const OPEN_BASS: [i32; 4] = [28, 33, 38, 43];
pub const OPEN_BASS5: [i32; 5] = [23, 28, 33, 38, 43];
pub const OPEN_BASS_NAMES: [&str; 4] = ["E", "A", "D", "G"];
pub const OPEN_BASS5_NAMES: [&str; 5] = ["B", "E", "A", "D", "G"];

/// The open strings (MIDI, low to high) and their names for what's played.
pub fn open_strings(inst: qrs_analysis::instrument::Instrument) -> (&'static [i32], &'static [&'static str]) {
    use qrs_analysis::instrument::Instrument;
    match inst {
        Instrument::Guitar => (&OPEN, &OPEN_NAMES),
        Instrument::Bass5 => (&OPEN_BASS5, &OPEN_BASS5_NAMES),
        _ => (&OPEN_BASS, &OPEN_BASS_NAMES),
    }
}

/// The open string a note (MIDI, fractional) is, within a quarter tone.
#[cfg(test)]
pub fn open_string(midi: f64) -> Option<usize> {
    open_string_of(&OPEN, midi)
}

/// The same among `strings`.
pub fn open_string_of(strings: &[i32], midi: f64) -> Option<usize> {
    strings.iter().position(|&m| (midi - m as f64).abs() < 0.5)
}

/// What the note check says, from how many strings it has heard.
#[cfg(test)]
pub fn notes_words(heard: usize, any_note: bool) -> &'static str {
    notes_words_of(heard, 6, any_note)
}

/// The same with `of` strings.
pub fn notes_words_of(heard: usize, of: usize, any_note: bool) -> &'static str {
    match heard {
        n if n >= of && of > 0 => "Every string heard: notes are detected.",
        n if n >= 3 => "Notes are detected. Play the rest if you like.",
        1 | 2 => "Heard you. Play the other open strings.",
        _ if any_note => "Notes are coming through. Play each open string.",
        _ => "Play each open string, one at a time, and let it ring.",
    }
}

// -- recording it -------------------------------------------------------------------------------------------------

/// Record how the setup went ("done" when finished); "done" isn't undone by a later "shown" or "dismissed" (the
/// fading first-run card of 0.3.0's candidates wrote those; the first-time setup replaced it, P23.14).
pub fn record(cfg: &mut Config, what: &str) {
    if cfg.get(KEY).and_then(Value::as_str) == Some("done") && what != "done" {
        return;
    }
    cfg.set(KEY, what);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lin(db: f32) -> f32 {
        10f32.powf(db / 20.0)
    }

    #[test]
    fn the_steps_go_forward_back_and_remember_skips() {
        let mut f = Flow::default();
        assert_eq!(f.step, Step::Input);
        assert!(!f.back()); // nothing before the first
        f.next();
        assert_eq!((f.step, f.mark(Step::Input)), (Step::Output, Mark::Done));
        f.skip();
        assert_eq!((f.step, f.mark(Step::Output)), (Step::Kind, Mark::Skipped));
        assert!(f.back());
        f.next(); // done after all
        assert_eq!(f.mark(Step::Output), Mark::Done);
        assert!(f.back());
        f.skip(); // a skip doesn't undo a step that was done
        assert_eq!(f.mark(Step::Output), Mark::Done);
        for _ in 0..10 {
            f.next();
        }
        assert_eq!(f.step, Step::Done); // it stays on the last
        assert_eq!(f.mark(Step::Done), Mark::Todo);
        for s in Step::ALL {
            assert_eq!(Step::parse(s.key()), Some(s));
            assert_eq!(Step::ALL[s.index()], s);
        }
        assert_eq!(Flow::at(Step::Notes).step, Step::Notes);
    }

    #[test]
    fn the_level_check_names_silence_quiet_clipping_and_good() {
        // no input
        let w = LevelWatch::default();
        assert_eq!(w.verdict(0.0, false), Level::NoInput);
        // listening, nothing played: wait, then say nothing's coming in
        let mut w = LevelWatch::default();
        for i in 0..400 {
            w.feed(i as f64 / 60.0, lin(-80.0), 0);
        }
        assert_eq!(w.verdict(1.0, true), Level::Waiting);
        assert_eq!(w.verdict(400.0 / 60.0, true), Level::Silent);
        // a quiet guitar, then a good one, then a hot one
        let mut w = LevelWatch::default();
        w.feed(0.0, lin(-42.0), 0);
        assert_eq!(w.verdict(0.0, true), Level::Quiet);
        w.feed(0.1, lin(-14.0), 0);
        assert_eq!(w.verdict(0.1, true), Level::Good);
        w.feed(0.2, lin(-2.5), 0);
        assert_eq!(w.verdict(0.2, true), Level::Hot);
        assert!(Level::Hot.ok() && !Level::Quiet.ok());
        // the window forgets: after a few seconds of good playing the hot peak is gone
        for i in 0..300 {
            w.feed(0.3 + i as f64 / 60.0, lin(-15.0), 0);
        }
        assert_eq!(w.verdict(5.3, true), Level::Good);
        // clipping: a peak at full scale, or the interface's clip count going up
        w.feed(6.0, lin(-0.2), 0);
        assert_eq!(w.verdict(6.0, true), Level::Clipping);
        let mut w = LevelWatch::default();
        w.feed(0.0, lin(-20.0), 3);
        w.feed(0.1, lin(-20.0), 4);
        assert_eq!(w.verdict(0.1, true), Level::Clipping);
        // once heard, a pause isn't "nothing coming in"
        for i in 0..600 {
            w.feed(0.2 + i as f64 / 60.0, lin(-90.0), 4);
        }
        assert_eq!(w.verdict(10.2, true), Level::Waiting);
        for l in [Level::Silent, Level::Quiet, Level::Clipping] {
            assert!(!l.words().1.is_empty());
        }
        // never a gain knob for a modeller (P36.6)
        use crate::calibrate::Kind;
        for l in [Level::Silent, Level::Quiet, Level::Hot, Level::Clipping] {
            assert!(!l.words_for(Kind::Modeller).1.contains("gain knob"), "{l:?}");
        }
        assert!(Level::Quiet.words_for(Kind::Mic).1.contains("closer"));
        assert_eq!(Level::Good.words_for(Kind::Modeller), Level::Good.words());
    }

    #[test]
    fn open_strings_are_told_apart() {
        assert_eq!(open_string(40.1), Some(0));
        assert_eq!(open_string(63.7), Some(5));
        assert_eq!(open_string(52.0), None); // a fretted note
        assert_eq!(open_string(45.6), None); // too far off
        assert!(notes_words(6, true).starts_with("Every string"));
        // a bass: E1 A1 D2 G2, a 5-string's low B0 too
        use qrs_analysis::instrument::Instrument;
        let (b, names) = open_strings(Instrument::Bass);
        assert_eq!((b, names.len()), (&OPEN_BASS[..], 4));
        assert_eq!(open_string_of(b, 28.2), Some(0));
        assert_eq!(open_string_of(open_strings(Instrument::Bass5).0, 23.1), Some(0));
        assert_eq!(notes_words_of(4, 4, true), notes_words(6, true));
        assert!(notes_words(0, false).starts_with("Play each"));
    }

    #[test]
    fn done_stays_done() {
        let mut cfg = Config::default();
        record(&mut cfg, "done");
        record(&mut cfg, "shown");
        assert_eq!(cfg.get(KEY).and_then(Value::as_str), Some("done"));
    }
}
