//! The coach (new in quickRS, P13.5, v2 P16.2-P16.4; no Python source): a lesson's item, step by step, in the
//! player's console where the tiles go. Each kind of item has its steps (`qrs_library::lessons::Kind`):
//! - by ear: listen (the riff looped with its notes hidden), find the first note and then the rest one by one (the
//!   song played up to the note you're after; each pluck read by qrs-analysis ear.rs and judged higher, lower,
//!   another octave or got it; hints when you're stuck), play along, then the tab;
//! - learn: listen with the tab, learn it (note by note on the loop: the song waits at each note until you play it),
//!   play along, done;
//! - scale: see the shape on the neck (the stretch's notes over it), run it up and down (each note heard and lit),
//!   play over the song (your notes on the neck, in the scale or out), done.
//!
//! Play along starts at full speed: Slower steps down the item's speeds, a good pass below full speed moves back up,
//! one at full speed finishes it. The Tuner key (U) swaps the step's card for the tuner whenever you want it.
//! Progress is written to userdata.json as it's made (D25, D28); an item learned is saved as a lick.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use qrs_analysis::ear::{PluckReader, Verdict, judge};
use qrs_audio::synth::SynthNote;
use qrs_formats::Techs;
use qrs_library::lessons::{Activity, Kind, in_scale, pc_name};
use qrs_library::{Lesson, Riff, RiffProgress};
use qrs_ui::icons::{self, Icon};
use qrs_ui::knobs;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{ButtonStyle, button, button_width, interact};

use super::{Chart, LOOP_LEAD_IN, PlayerReq, PlayerView};
use crate::library::{caps, txt};
use crate::state::AppState;

/// How long a verdict stays up after its pluck (seconds).
const SAID_FOR: f32 = 2.5;
/// A breath after a find before the song plays up to the next note.
const NEXT_AFTER: f32 = 0.9;
/// Cents from an open string that count as in tune, and how long it has to stay there.
const IN_TUNE: f64 = 7.0;
const TUNE_HOLD: f32 = 0.4;
/// Notes this far apart (seconds) start a new group in the phrase.
const GROUP_GAP: f64 = 0.65;
/// How far back (seconds) "Hear it" starts before the note you're after (from the first note within it).
const CONTEXT: f64 = 4.0;
/// "Hear it" slowed down.
const SLOW: f64 = 0.7;
/// Over the song: how long a note you played stays on the neck (seconds), and how many at most.
const JAM_FOR: f32 = 3.0;
const JAM_KEEP: usize = 16;
/// Seconds a note of the scale takes when the synth plays it up and down.
const RUN_NOTE: f64 = 0.32;

/// The steps of an item, of every kind (each kind takes some of them, `Step::list`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Listen,
    First,
    Notes,
    Play,
    Check,
    Practise,
    Record,
    Done,
    See,
    Run,
    Jam,
    /// A theory item's page.
    Page(usize),
}

impl Step {
    /// An item's steps, in order (as `Riff::step_list`: a theory item's pages).
    pub fn list(riff: &Riff) -> Vec<Step> {
        match riff.kind {
            Kind::Ear => vec![Step::Listen, Step::First, Step::Notes, Step::Play, Step::Check],
            Kind::Learn => vec![Step::Listen, Step::Practise, Step::Record, Step::Done],
            Kind::Scale => vec![Step::See, Step::Run, Step::Jam, Step::Done],
            Kind::Theory => (0..riff.pages.len().max(1)).map(Step::Page).collect(),
        }
    }

    /// The step stored as `k` for this item (v1's "tune", or one the kind hasn't got: its first; v2's "learn" and
    /// "play" for a learn item: Practise).
    pub fn from_key(riff: &Riff, k: &str) -> Step {
        let steps = Step::list(riff);
        steps[riff.step_index(k).min(steps.len() - 1)]
    }

    /// Its key in the progress.
    pub fn key(self) -> String {
        match self {
            Step::Listen => "listen",
            Step::First => "first",
            Step::Notes => "notes",
            Step::Play => "play",
            Step::Check => "check",
            Step::Practise => "practise",
            Step::Record => "record",
            Step::Done => "done",
            Step::See => "see",
            Step::Run => "run",
            Step::Jam => "jam",
            Step::Page(i) => return qrs_library::lessons::page_key(i),
        }
        .into()
    }

    /// Its name (a page's: `Coach::step_title`).
    pub fn title(self) -> &'static str {
        match self {
            Step::Listen => "Listen",
            Step::First => "The first note",
            Step::Notes => "Note by note",
            Step::Play => "Play along",
            Step::Check => "Check the tab",
            Step::Practise => "Practise",
            Step::Record => "Record",
            Step::Done => "Done",
            Step::See => "See it",
            Step::Run => "Up and down",
            Step::Jam => "Over the song",
            Step::Page(_) => "Page",
        }
    }

    /// The item's last step: it's been learned.
    fn last(self) -> bool {
        matches!(self, Step::Check | Step::Done)
    }
}

/// A note of the phrase to find.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub t: f64,
    pub string: i32,
    pub fret: i32,
    pub midi: i32,
    pub sus: f64,
}

/// The phrase: the riff's single notes from its start (the first `phrase` of them; muted notes have no pitch to find).
pub fn phrase_of(chart: &Chart, riff: &Riff) -> Vec<Target> {
    let i0 = chart.ntimes.partition_point(|&t| t < riff.start - 0.02);
    let mut out = Vec::new();
    for n in &chart.single_notes[i0..] {
        if n.time >= riff.end - 0.01 {
            break;
        }
        if n.techniques.has(Techs::FRET_HAND_MUTE) {
            continue;
        }
        let Some(open) = chart.opens.get(n.string as usize) else {
            continue;
        };
        out.push(Target {
            t: n.time,
            string: n.string,
            fret: n.fret,
            midi: open + n.fret,
            sus: n.sustain,
        });
        if riff.phrase > 0 && out.len() == riff.phrase {
            break;
        }
    }
    out
}

/// Every note of the stretch (single notes and chords' notes; not muted ones) as (string, fret), each place once.
pub fn stretch_spots(chart: &Chart, riff: &Riff) -> Vec<(i32, i32)> {
    let i0 = chart.gtimes.partition_point(|&t| t < riff.start - 0.02);
    let mut out: Vec<(i32, i32)> = Vec::new();
    for g in &chart.glyphs[i0..] {
        if g.t >= riff.end - 0.01 {
            break;
        }
        if g.tech.has(Techs::FRET_HAND_MUTE) || g.fret < 0 || out.contains(&(g.string, g.fret)) {
            continue;
        }
        out.push((g.string, g.fret));
    }
    out
}

/// What "Hear it" plays for note `k` of the phrase (chart seconds): from the first note within `CONTEXT` seconds before
/// it (just the note, for the first), to just before the note after it, so the note you're after is the last thing
/// you hear.
pub fn hear_span(phrase: &[Target], k: usize) -> Option<(f64, f64)> {
    let tg = phrase.get(k)?;
    let j = (0..=k).find(|&j| tg.t - phrase[j].t <= CONTEXT).unwrap_or(k);
    let a = if j == k { tg.t - 0.08 } else { phrase[j].t - 0.15 };
    let ring = if k + 1 == phrase.len() {
        tg.sus.clamp(0.5, 1.2)
    } else {
        tg.sus.clamp(0.35, 0.8)
    };
    let b = match phrase.get(k + 1) {
        Some(n) if n.t - tg.t < ring + 0.03 => n.t - 0.03,
        _ => tg.t + ring,
    };
    Some((a.max(0.0), b.max(tg.t + 0.12)))
}

/// What a pluck was while looking for note `k` of the phrase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// Judged against the note being looked for.
    Judged(Verdict),
    /// The next note of a run-through of the notes already found (how far it's got): people find a note by playing
    /// the riff from the start up to it, and those notes aren't wrong.
    Through(usize),
}

/// A pluck (`played`, MIDI) while looking for note `k`, with a run-through `run` notes along (0: none going): the
/// run-through's next note keeps it going; else the note looked for is found; else the riff's first note starts a
/// run-through (when there's one to play); anything else is judged against the note looked for.
pub fn hear_pluck(phrase: &[Target], k: usize, run: usize, played: f64) -> Heard {
    let m = played.round() as i32;
    if run > 0 && run < k && phrase[run].midi == m {
        return Heard::Through(run + 1);
    }
    let v = judge(played, phrase[k].midi);
    if v == Verdict::Match {
        return Heard::Judged(v);
    }
    if k > 0 && phrase[0].midi == m {
        return Heard::Through(1);
    }
    Heard::Judged(v)
}

/// What a pluck was in a run up and down the scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunHeard {
    /// The note due: on to the next.
    Next,
    /// The run's last note: up and down done.
    Done,
    /// The note just played again (a second pluck, a re-pick): nothing changes.
    Again,
    /// The run's first note, part way through: starting over.
    Over,
    /// Another note: the one due is still due.
    Wrong,
}

/// A pluck (`played`, MIDI) at note `pos` of the run `seq` (`Scale::run`): the same pitch as the note due counts
/// wherever it's fretted.
pub fn run_pluck(seq: &[i32], pos: usize, played: f64) -> RunHeard {
    let m = played.round() as i32;
    match seq.get(pos) {
        Some(&due) if due == m => {
            if pos + 1 == seq.len() {
                RunHeard::Done
            } else {
                RunHeard::Next
            }
        }
        _ if pos > 0 && seq[pos - 1] == m => RunHeard::Again,
        _ if pos > 0 && seq.first() == Some(&m) => RunHeard::Over,
        _ => RunHeard::Wrong,
    }
}

/// The hints for the note being looked for, gentlest first: the string (when it wasn't given), how far from the note
/// before (which frets, for the first note), then the fret itself.
pub fn hints_for(tg: &Target, prev: Option<&Target>, string_given: bool, string: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !string_given {
        out.push(format!("It's on the {string}."));
    }
    match prev {
        Some(p) => {
            let d = tg.midi - p.midi;
            if d == 0 {
                out.push("It's the same note as the one before.".into());
            } else {
                let dir = if d > 0 { "higher" } else { "lower" };
                let size = if d.abs() <= 2 { "a small step" } else { "a jump" };
                out.push(format!("It's {dir} than the note before: {size}."));
                let n = d.abs();
                let unit = if tg.string == p.string { "fret" } else { "semitone" };
                out.push(format!(
                    "It's {n} {unit}{} {dir} than the note before.",
                    if n == 1 { "" } else { "s" }
                ));
            }
        }
        None => {
            let lo = (tg.fret - 2).max(0);
            out.push(format!(
                "It's between fret {lo} and fret {} on the {string}.",
                lo + 4
            ));
        }
    }
    out.push(if tg.fret == 0 {
        format!("It's the open {string}.")
    } else {
        format!("It's fret {} on the {string}.", tg.fret)
    });
    out.dedup();
    out
}

/// The frets a scale's box is drawn over (first, last): the shape's with a fret either side, the stretch's notes
/// too when they're near it, at least five frets, from the nut when it's that close.
pub fn box_frets(shape: &[(i32, i32)], stretch: &[(i32, i32)]) -> (i32, i32) {
    let Some(lo) = shape.iter().map(|s| s.1).min() else {
        return (0, 5);
    };
    let hi = shape.iter().map(|s| s.1).max().unwrap_or(lo);
    let near: Vec<i32> = stretch
        .iter()
        .map(|s| s.1)
        .filter(|&f| f >= lo - 3 && f <= hi + 3)
        .collect();
    let lo = near.iter().copied().fold(lo, i32::min);
    let hi = near.iter().copied().fold(hi, i32::max);
    let mut a = (lo - 1).max(0);
    if a <= 1 {
        a = 0;
    }
    let b = (hi + 1).max(a + 5).min(24);
    (a, b)
}

/// An item being learned: where it's got to, what the coach last said, and what's being listened for.
pub struct Coach {
    pub lesson: Arc<Lesson>,
    pub riff: Riff,
    /// Its place in the lesson.
    pub index: usize,
    pub step: Step,
    /// Its steps (a theory item's pages).
    pub steps: Vec<Step>,
    pub phrase: Vec<Target>,
    pub progress: RiffProgress,
    pub reader: PluckReader,
    /// The last note judged: what was played (MIDI), the verdict, when.
    pub said: Option<(f64, Verdict, Instant)>,
    /// A pluck whose note couldn't be read: when.
    pub(super) unclear: Option<Instant>,
    /// A run-through of the notes found: how far it's got, and its last note (when).
    run: usize,
    through: Option<Instant>,
    /// Hints given for the note being looked for.
    hints: usize,
    /// The loop passes counted when the Listen step began.
    listen_base: u32,
    /// The play-along speeds, fastest first; the one played at (an index into them), and the last pass's accuracy.
    pub ladder: Vec<f64>,
    pub speed_i: usize,
    pub(super) last_pass: Option<Option<f64>>,
    /// The tuner is up in place of the step's card.
    pub tuner: bool,
    /// The tuner: each open string heard in tune, and the one being held in tune (since when).
    pub tuned: Vec<bool>,
    tune_hold: Option<(usize, Instant)>,
    /// Practise: note by note on (the song waits at each note: Phase 15's mode, W), and whether the pass under way
    /// had it on at any point (it's a practice pass then, not counted).
    pub nbn: bool,
    pub(super) nbn_pass: bool,
    /// Record: the take (`practise.rs`).
    pub take: super::practise::Take,
    /// A whole-song item: its bars unlike anything learned in the lesson's earlier items (`practise.rs`).
    pub novel: Vec<qrs_analysis::similar::NovelBar>,
    /// A theory item: the stretch's notes weighed (`theory::emphasis`), what's being found on the page (pitches,
    /// found yet), the last pluck there (what, which target, when), and how far its text is scrolled.
    pub emph: super::theory::Emphasis,
    pub find: Vec<(i32, bool)>,
    pub(super) find_said: Option<(f64, Option<usize>, Instant)>,
    /// The page `find` is for (a refit keeps what's been found on it), and the runs up and down on this visit.
    pub(super) find_page: Option<usize>,
    pub page_runs: u32,
    pub sheet_scroll: f32,
    /// A scale: its notes up and down (MIDI), how far the run has got, whether it's had a wrong note, and the last
    /// pluck's verdict (what was played, when).
    pub run_seq: Vec<i32>,
    pub run_pos: usize,
    pub(super) run_wrong: bool,
    pub(super) run_said: Option<(f64, RunHeard, Instant)>,
    /// A scale's pitch classes (`Scale::pitch_classes` on the chart's tuning).
    pub pcs: u16,
    /// The stretch's notes (string, fret), drawn over the scale's box.
    pub stretch: Vec<(i32, i32)>,
    /// Over the song: the notes you played lately (MIDI, when), and how many of them were in the scale.
    pub jam: Vec<(i32, Instant)>,
    pub jam_in: u32,
    pub jam_all: u32,
    /// "Hear it" slowed down.
    pub slow: bool,
    /// When to play the song up to the next note (a breath after a find).
    hear_at: Option<Instant>,
    /// Progress changed: written to userdata.json on the next frame.
    pub(super) dirty: bool,
    since: Instant,
    saved_lick: bool,
    /// The console shows the coach (else the tiles or the controls).
    pub show: bool,
}

impl Coach {
    pub(super) fn new(lesson: Arc<Lesson>, index: usize, riff: Riff, progress: RiffProgress) -> Coach {
        let steps = Step::list(&riff);
        Coach {
            lesson,
            index,
            step: steps[0],
            steps,
            phrase: Vec::new(),
            progress,
            reader: PluckReader::default(),
            said: None,
            unclear: None,
            run: 0,
            through: None,
            hints: 0,
            listen_base: 0,
            ladder: riff.ladder(),
            speed_i: 0,
            last_pass: None,
            tuner: false,
            tuned: Vec::new(),
            tune_hold: None,
            nbn: false,
            nbn_pass: false,
            take: Default::default(),
            novel: Vec::new(),
            emph: Default::default(),
            find: Vec::new(),
            find_said: None,
            find_page: None,
            page_runs: 0,
            sheet_scroll: 0.0,
            run_seq: Vec::new(),
            run_pos: 0,
            run_wrong: false,
            run_said: None,
            pcs: 0,
            stretch: Vec::new(),
            jam: Vec::new(),
            jam_in: 0,
            jam_all: 0,
            slow: false,
            hear_at: None,
            dirty: true,
            since: Instant::now(),
            saved_lick: false,
            show: true,
            riff,
        }
    }

    /// Read what it needs from the part's chart: the phrase to find, the stretch's notes, the scale on its tuning
    /// (Some(why) when the shape doesn't go up in pitch on it).
    fn fit(&mut self, chart: &Chart) -> Option<String> {
        self.phrase = phrase_of(chart, &self.riff);
        self.progress.found = self.progress.found.min(self.phrase.len() as u32);
        self.stretch = stretch_spots(chart, &self.riff);
        self.tuned = vec![false; chart.opens.len()];
        if self.riff.kind == Kind::Theory {
            self.emph = super::theory::emphasis_of(chart, self.riff.start, self.riff.end);
            if let Step::Page(i) = self.step {
                return self.fit_page(chart, i);
            }
            return None;
        }
        let sc = self.riff.scale.as_ref()?;
        self.run_seq = sc.run(&chart.opens);
        self.run_pos = self.run_pos.min(self.run_seq.len().saturating_sub(1));
        self.pcs = sc.pitch_classes(&chart.opens);
        sc.out_of_order(&chart.opens).map(|i| {
            let (s, f) = sc.shape[i];
            format!(
                "{}: the shape's note {} ([{s}, {f}]) isn't higher than the one before on this tuning",
                self.riff.id,
                i + 1
            )
        })
    }

    /// Another part (or difficulty) of the song: its notes read again (the tuner as it was).
    pub fn refit(&mut self, chart: &Chart) {
        let tuned = std::mem::take(&mut self.tuned);
        self.fit(chart);
        if tuned.len() == self.tuned.len() {
            self.tuned = tuned;
        }
    }

    /// The note being looked for (its index in the phrase), if there's one left.
    pub fn looking_for(&self) -> Option<usize> {
        let k = self.progress.found as usize;
        (matches!(self.step, Step::First | Step::Notes) && k < self.phrase.len()).then_some(k)
    }

    pub fn all_found(&self) -> bool {
        !self.phrase.is_empty() && self.progress.found as usize >= self.phrase.len()
    }

    /// The strings the phrase is played on (low to high).
    fn strings(&self) -> Vec<i32> {
        let mut s: Vec<i32> = self.phrase.iter().map(|n| n.string).collect();
        s.sort();
        s.dedup();
        s
    }

    /// The play-along speed.
    pub(super) fn speed(&self) -> f64 {
        self.ladder.get(self.speed_i).copied().unwrap_or(1.0)
    }

    /// A step's name (a page's title).
    pub fn step_title(&self, s: Step) -> String {
        match s {
            Step::Page(i) => self
                .riff
                .pages
                .get(i)
                .map(|p| p.title.clone())
                .unwrap_or_default(),
            s => s.title().to_string(),
        }
    }

    /// The step's place in the kind's steps.
    pub(super) fn step_no(&self) -> usize {
        self.steps.iter().position(|s| *s == self.step).unwrap_or(0)
    }

    /// The step after this one (None after the last).
    fn next_step(&self) -> Option<Step> {
        self.steps.get(self.step_no() + 1).copied()
    }

    /// Whether the run up and down is past its top note (coming down).
    pub fn run_down(&self) -> bool {
        let up = self.run_seq.len().div_ceil(2);
        self.run_pos >= up
    }
}

/// Speed ladder: one notch slower (`d` 1) or faster (-1), within the ladder (fastest first).
pub fn notch(speed_i: usize, len: usize, d: i32) -> usize {
    (speed_i as i32 + d).clamp(0, len.saturating_sub(1) as i32) as usize
}

/// A play-along pass of `acc` at `speed_i` of a ladder (fastest first) with this `pass` mark: the speed to go on at,
/// or None when it was a pass at the top (the item's finished).
pub fn after_pass(speed_i: usize, acc: f64, pass: f64) -> Option<usize> {
    if acc < pass {
        Some(speed_i)
    } else if speed_i > 0 {
        Some(speed_i - 1)
    } else {
        None
    }
}

fn epoch_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn pct(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

impl PlayerView {
    /// Open the coach on a lesson's item (this player's song): its stretch looped, carrying on at the step you left
    /// it at (an item done before starts again from its first step).
    pub fn start_lesson(&mut self, st: &mut AppState, lesson: Arc<Lesson>, riff_id: &str) {
        let Some(index) = lesson.riffs.iter().position(|r| r.id == riff_id) else {
            return;
        };
        let riff = lesson.riffs[index].clone();
        let mut progress = st.userdata.riff_progress(&lesson.id, &riff.id);
        if progress.started <= 0.0 {
            progress.started = epoch_now();
        }
        let kind = riff.kind;
        let mut step = if progress.done {
            progress.found = 0;
            Step::list(&riff)[0]
        } else {
            Step::from_key(&riff, &progress.step)
        };
        let mut c = Coach::new(lesson, index, riff.clone(), progress);
        let bad_shape = c.fit(&self.chart);
        if step == Step::Notes && c.progress.found == 0 {
            step = Step::First;
        }
        let found: Vec<(f64, i32)> = c.phrase[..c.progress.found as usize]
            .iter()
            .map(|n| (n.t, n.string))
            .collect();
        let no_phrase = kind == Kind::Ear && c.phrase.is_empty();
        self.coach = Some(c);
        self.coach_novel();
        self.loop_a = Some(riff.start);
        self.loop_b = Some(riff.end);
        self.loop_passes = 0;
        self.loop_acc = None;
        self.revealed = vec![false; self.chart.glyphs.len()]; // (the item before's notes, on the same song)
        self.set_ear(kind == Kind::Ear);
        for (t_, s_) in found {
            self.reveal_note(t_, s_);
        }
        self.coach_enter(st, step);
        if no_phrase {
            self.say_for(
                "This chart has no notes where the lesson expects the riff: ",
                "listen and play along still work",
                4.0,
            );
        }
        if let Some(why) = bad_shape {
            crate::applog::write(&format!("lesson {}: {why}", st_lesson_id(self)));
            self.say_for(
                "The lesson's scale shape doesn't go up in pitch on this part's tuning: ",
                "see quickrs.log",
                4.0,
            );
        }
    }

    /// Stop what's playing (the song, a count-in, the selection player).
    pub(super) fn coach_quiet(&mut self) {
        self.take_listen_stop();
        self.countdown_until = None;
        if self.transport.playing() {
            self.transport.pause();
        }
        self.stop_synth();
    }

    /// Play at `speed` (applied on the next frame, like the speed keys after their pause).
    pub(super) fn coach_speed(&mut self, speed: f64) {
        if (self.speed() - speed).abs() > 1e-6 {
            self.pending_speed = Some(speed);
            self.speed_at = Instant::now() - Duration::from_secs(1);
        }
    }

    /// The stretch from its lead-in, counted in.
    pub(super) fn coach_count_in(&mut self) {
        let Some(a) = self.coach.as_ref().map(|c| c.riff.start) else {
            return;
        };
        self.seek((a - LOOP_LEAD_IN).max(0.0));
        self.count_in();
        self.run_scored = false;
    }

    /// Go to a step, setting the song up for it.
    pub(super) fn coach_enter(&mut self, st: &mut AppState, step: Step) {
        if step != Step::Done && self.take.as_ref().is_some_and(super::take::Session::going) {
            self.take_abort("Take stopped");
        }
        let Some(c) = self.coach.as_mut() else { return };
        let step = match step {
            Step::First | Step::Notes if c.phrase.is_empty() => Step::Play,
            Step::Notes if c.progress.found == 0 => Step::First,
            s => s,
        };
        let from = std::mem::replace(&mut c.step, step);
        c.said = None;
        c.unclear = None;
        c.run = 0;
        c.through = None;
        c.hints = 0;
        c.hear_at = None;
        c.tuner = false;
        c.tune_hold = None;
        c.last_pass = None;
        c.run_pos = 0;
        c.run_wrong = false;
        c.run_said = None;
        c.jam.clear();
        c.reader.clear();
        c.show = true;
        c.dirty = true;
        let a = c.riff.start;
        let ear = c.riff.kind == Kind::Ear;
        if step == Step::Play || (step == Step::Practise && from != Step::Record) {
            c.speed_i = 0; // play along and practise start at full speed (back from a take: as it was)
        }
        if step != Step::Practise {
            c.nbn = false;
        }
        c.nbn_pass = c.nbn;
        c.take = Default::default();
        c.find_said = None;
        c.sheet_scroll = 0.0;
        let play_speed = c.speed();
        self.coach_quiet();
        match step {
            Step::Listen => {
                self.set_ear(ear);
                self.coach_speed(1.0);
                self.seek((a - LOOP_LEAD_IN).max(0.0));
                self.transport.play();
                let passes = self.loop_passes;
                if let Some(c) = self.coach.as_mut() {
                    c.listen_base = passes;
                }
            }
            Step::First | Step::Notes => {
                self.set_ear(true);
                self.seek(a);
                if let Some(c) = self.coach.as_mut() {
                    c.hear_at = Some(Instant::now() + Duration::from_millis(500));
                }
            }
            Step::Play => {
                self.set_ear(ear);
                self.coach_speed(play_speed);
                self.coach_count_in();
            }
            Step::Practise => {
                // (note by note, when it's on, is nbn_watch's)
                self.set_ear(false);
                self.coach_speed(play_speed);
                self.coach_count_in();
            }
            Step::Record => {
                // (the take starts with its key: practise.rs)
                self.set_ear(false);
                self.coach_speed(play_speed);
                self.seek((a - LOOP_LEAD_IN).max(0.0));
            }
            Step::Page(i) => self.page_enter(i),
            Step::Jam => {
                self.set_ear(true);
                self.coach_speed(1.0);
                self.coach_count_in();
                if let Some(c) = self.coach.as_mut() {
                    c.jam_in = 0;
                    c.jam_all = 0;
                }
            }
            Step::See | Step::Run => {
                self.set_ear(false);
                self.coach_speed(1.0);
                self.seek((a - LOOP_LEAD_IN).max(0.0));
            }
            Step::Check | Step::Done => {
                self.set_ear(false);
                self.coach_speed(1.0);
                self.seek(a);
                self.coach_finish(st);
            }
        }
    }

    /// The tuner up in place of the step's card (the song stopped), or the step back.
    fn coach_tuner(&mut self) {
        let Some(c) = self.coach.as_mut() else { return };
        c.tuner = !c.tuner;
        c.show = true;
        c.tune_hold = None;
        c.reader.clear();
        if c.tuner {
            c.tuned.iter_mut().for_each(|t_| *t_ = false);
            self.coach_quiet();
            self.nbn_release(false);
        }
    }

    /// Play along (practise, a take) a notch slower (`d` 1) or faster (-1).
    fn coach_notch(&mut self, d: i32) {
        let Some(c) = self.coach.as_mut() else { return };
        if !matches!(c.step, Step::Play | Step::Practise | Step::Record) || c.take.going() {
            return;
        }
        let i = notch(c.speed_i, c.ladder.len(), d);
        if i == c.speed_i {
            let s = c.speed();
            self.say(
                format!(
                    "Already at {}%{}",
                    pct(s),
                    if d > 0 { ", the slowest" } else { "" }
                ),
                "",
            );
            return;
        }
        c.speed_i = i;
        c.last_pass = None;
        let s = c.speed();
        let what = match c.step {
            Step::Practise => "Practise",
            Step::Record => "Record",
            _ => "Play along",
        };
        self.coach_speed(s);
        self.say(format!("{what} at {}%", pct(s)), "");
    }

    /// Screenshot mode: the coach on this song as if an item of the first lesson were here (its stretch the 8 s before
    /// the shot's moment), at a step: "tuner", "notes", "play", "slower" (play along a notch down), "check",
    /// "practise", "practise_nbn" (note by note on), "record" (a take under way), "record_result" (one that
    /// didn't pass), "callout" (listening, a callout up), "learn_done", "see", "run", "jam" or "done" (a scale's);
    /// or a theory item's page ("theory_key", "theory_chords", "theory_scale" (a run), "theory_emphasis",
    /// "theory_find", "theory_jam"; the built-in lessons' first theory item, its pages over this song). Nothing of it
    /// is written to the user data.
    pub(super) fn stage_coach(&mut self, st: &AppState, step: &str) {
        let theory = step.starts_with("theory_");
        let pick = if theory {
            st.lessons
                .iter()
                .find_map(|l| Some((l, l.riffs.iter().position(|r| r.kind == Kind::Theory)?)))
        } else {
            st.lessons.first().map(|l| (l, 0))
        };
        let Some((l, ri)) = pick else { return };
        let mut lesson = (**l).clone();
        lesson.id = "screenshots".into();
        let now = self.now();
        let Some(r) = lesson.riffs.get_mut(ri) else {
            return;
        };
        r.start = now - 9.0;
        r.end = now - 1.0;
        r.phrase = 10;
        r.title = self.entry.display_title();
        r.artist = self.entry.artist.clone();
        r.part = self.part_name.clone();
        if let Some(which) = step.strip_prefix("whole_") {
            self.stage_whole(st, which);
            return;
        }
        let kind = match step {
            "practise" | "practise_nbn" | "record" | "record_result" | "record_report"
            | "record_analysing" | "callout" | "learn_done" => Kind::Learn,
            "see" | "run" | "jam" | "done" => Kind::Scale,
            _ if theory => Kind::Theory,
            _ => Kind::Ear,
        };
        r.kind = kind;
        r.callouts.clear();
        if kind == Kind::Learn {
            r.pass = 0.85;
            r.name = "The intro".into();
            r.callouts = vec![
                qrs_library::lessons::Callout {
                    at: now - 6.0,
                    text: "The double stops start here: one finger flat across the G and B strings".into(),
                },
                qrs_library::lessons::Callout {
                    at: now + 0.8,
                    text:
                        "Chorus 2: unison bends instead of the stabs you learned, the same notes an octave up"
                            .into(),
                },
            ];
        }
        if matches!(kind, Kind::Learn | Kind::Scale) {
            r.how = vec![
                "Fret each note with the tip of your finger, just behind the fret.".into(),
                "Keep your first finger at the box's first fret: each finger has its fret.".into(),
                "Pick down on the beat, up in between.".into(),
                "Let the open strings ring only where the record does.".into(),
                "Mute the strings you aren't playing with the side of your picking hand, and lift the fretting finger \
                 just off the string between notes so they stop clean."
                    .into(),
                "Say the rhythm out loud before you play it: long, short short, long.".into(),
            ];
        }
        if kind == Kind::Scale {
            let (name, root, shape) = shot_scale(&self.chart, r.start, r.end);
            r.name = format!("{name}, box 1");
            r.scale = Some(qrs_library::lessons::Scale { name, root, shape });
        }
        // a theory shot's page: the first with what the shot is of
        let page = if theory {
            let has = |f: &dyn Fn(&qrs_library::lessons::Page) -> bool| r.pages.iter().position(f);
            match step {
                "theory_chords" => has(&|p| !p.show.chords.is_empty() && !p.show.neck()),
                "theory_scale" => has(&|p| p.act == Activity::Run && p.show.intervals),
                "theory_emphasis" => has(&|p| p.show.emphasis),
                "theory_find" => has(&|p| p.act == Activity::Find && p.show.scale.is_some())
                    .or_else(|| has(&|p| p.act == Activity::Find)),
                "theory_jam" => has(&|p| p.act == Activity::Jam),
                _ => Some(0),
            }
            .unwrap_or(0)
        } else {
            0
        };
        let riff = r.clone();
        lesson.riffs[ri] = riff.clone();
        let progress = RiffProgress {
            started: epoch_now() - 900.0,
            listens: 5,
            ..Default::default()
        };
        let mut c = Coach::new(Arc::new(lesson), ri, riff.clone(), progress);
        if theory {
            c.step = Step::Page(page);
        }
        c.fit(&self.chart);
        c.dirty = false;
        let n = c.phrase.len();
        match step {
            "tuner" => {
                c.step = Step::Notes;
                c.progress.found = 4.min(n) as u32;
                c.tuner = true;
                for t_ in c.tuned.iter_mut().take(3) {
                    *t_ = true;
                }
            }
            "notes" => {
                c.step = Step::Notes;
                c.progress.found = 4.min(n) as u32;
                c.progress.wrong = 7;
                if let Some(tg) = c.phrase.get(c.progress.found as usize) {
                    c.said = Some((tg.midi as f64 - 3.1, Verdict::Off(3), Instant::now()));
                }
                c.hints = 1;
                c.progress.hints = 1;
            }
            "play" | "slower" => {
                c.step = Step::Play;
                c.progress.found = n as u32;
                c.progress.record(1.0, 0.72);
                if step == "slower" {
                    c.progress.record(0.85, 0.84);
                    c.speed_i = 1;
                    c.last_pass = Some(Some(0.84));
                } else {
                    c.last_pass = Some(Some(0.72));
                }
            }
            "practise" | "practise_nbn" => {
                c.step = Step::Practise;
                c.progress.record(1.0, 0.78);
                c.progress.record(0.85, 0.91);
                c.speed_i = 1;
                c.last_pass = Some(Some(0.91));
                c.nbn = step == "practise_nbn";
            }
            "record" => {
                c.step = Step::Record;
                c.progress.record(1.0, 0.81);
                c.progress.takes = 1;
                c.take = super::practise::Take::Recording(Instant::now());
            }
            "record_report" => {
                c.step = Step::Record;
                c.progress.takes = 2;
                c.take = super::practise::Take::Done(super::practise::take_outcome(Some(0.81), 1.0, 0.85));
            }
            "record_analysing" => {
                c.step = Step::Record;
                c.progress.takes = 1;
                c.take = super::practise::Take::Analysing;
            }
            "record_result" => {
                c.step = Step::Record;
                c.progress.record(1.0, 0.81);
                c.progress.takes = 2;
                c.take = super::practise::Take::Done(super::practise::take_outcome(Some(0.81), 1.0, 0.85));
            }
            "callout" => c.step = Step::Listen,
            "learn_done" => {
                c.step = Step::Done;
                c.progress.seconds = 840.0;
                c.progress.done = true;
                c.progress.record(1.0, 0.93);
                c.progress.record(0.85, 0.95);
                c.progress.takes = 3;
                c.take = super::practise::Take::Done(super::practise::take_outcome(Some(0.93), 1.0, 0.85));
            }
            "see" => c.step = Step::See,
            "run" | "theory_scale" => {
                if !theory {
                    c.step = Step::Run;
                }
                c.run_pos = (c.run_seq.len() / 2 + 2).min(c.run_seq.len().saturating_sub(1));
                c.progress.runs = 2;
                c.progress.clean = 1;
                if let Some(&due) = c.run_seq.get(c.run_pos) {
                    c.run_said = Some((due as f64 + 1.0, RunHeard::Wrong, Instant::now()));
                }
            }
            "jam" | "theory_jam" => {
                if !theory {
                    c.step = Step::Jam;
                }
                let t0 = Instant::now();
                let up: Vec<i32> = c
                    .run_seq
                    .iter()
                    .take(c.run_seq.len().div_ceil(2))
                    .copied()
                    .collect();
                for (k, m) in up.iter().enumerate().skip(2).take(5) {
                    c.jam.push((*m, t0 - Duration::from_millis(400 * k as u64)));
                }
                if let Some(m) = up.get(3) {
                    c.jam.push((m + 1, t0 - Duration::from_millis(200)));
                }
                c.jam_in = 17;
                c.jam_all = 21;
            }
            "theory_find" => {
                if let Some(f) = c.find.first_mut() {
                    f.1 = true;
                }
                if let Some(&(m, _)) = c.find.first() {
                    c.find_said = Some((m as f64, Some(0), Instant::now()));
                }
            }
            "done" => {
                c.step = Step::Done;
                c.progress.runs = 4;
                c.progress.clean = 2;
                c.jam_in = 41;
                c.jam_all = 50;
                c.progress.seconds = 600.0;
                c.progress.done = true;
            }
            _ if theory => {}
            _ => {
                c.step = Step::Check;
                c.progress.found = n as u32;
                c.progress.wrong = 14;
                c.progress.octave = 2;
                c.progress.hints = 1;
                c.progress.seconds = 1260.0;
                c.progress.done = true;
                c.progress.record(1.0, 0.94);
            }
        }
        let found: Vec<(f64, i32)> = c.phrase[..c.progress.found as usize]
            .iter()
            .map(|t_| (t_.t, t_.string))
            .collect();
        let ear = matches!(c.step, Step::Notes | Step::Play | Step::Jam) && kind == Kind::Ear
            || matches!(c.step, Step::Jam);
        let nbn = c.nbn;
        self.coach = Some(c);
        self.loop_a = Some(riff.start);
        self.loop_b = Some(riff.end);
        // (the shot's judged notes would show in ear mode: only the ones found do)
        self.restart_counters();
        self.states.clear();
        self.revealed = vec![false; self.chart.glyphs.len()];
        self.set_ear(ear);
        for (t_, s_) in found {
            self.reveal_note(t_, s_);
        }
        if nbn {
            self.nbn_shot(false);
        }
        match step {
            "record_report" => self.stage_take("lesson"),
            "record_analysing" => self.stage_take("lesson_analysing"),
            _ => {}
        }
    }

    /// Screenshot mode: the built-in Johnny B. Goode's whole-song item ("lead" or "rhythm") practising, its "new
    /// here" bars on the tab, at its first one (or QUICKRS_SHOT_AT's moment when that's past it). Needs that song.
    fn stage_whole(&mut self, st: &AppState, which: &str) {
        let id = if which == "rhythm" {
            "whole-song-rhythm"
        } else {
            "whole-song"
        };
        let Some(l) = st.lessons.iter().find(|l| l.riffs.iter().any(|r| r.id == id)) else {
            return;
        };
        let Some(index) = l.riffs.iter().position(|r| r.id == id) else {
            return;
        };
        let riff = l.riffs[index].clone();
        if !riff.is_song(&self.entry.artist, &self.entry.display_title()) {
            return;
        }
        if let Some(i) = riff.part_of(&self.entry.arrangements) {
            let key = self.entry.arrangements[i].key.clone();
            if key != self.part_key {
                self.load_chart(st, &key, 1.0);
            }
        }
        let progress = RiffProgress {
            started: epoch_now() - 900.0,
            listens: 1,
            ..Default::default()
        };
        let mut c = Coach::new(l.clone(), index, riff.clone(), progress);
        c.fit(&self.chart);
        c.step = Step::Practise;
        c.dirty = false;
        self.coach = Some(c);
        self.coach_novel();
        self.loop_a = Some(riff.start);
        self.loop_b = Some(riff.end);
        let first = self
            .coach
            .as_ref()
            .and_then(|c| c.novel.first().map(|n| n.t0))
            .unwrap_or(riff.start);
        self.transport.pause();
        self.seek((first + 0.05 + self.offset).max(0.0));
        self.tab.jump = true;
        self.restart_counters();
        self.states.clear();
        self.set_ear(false);
    }

    /// The item is done: marked so, and (learned from the record, not a scale) saved as a lick, once.
    pub(super) fn coach_finish(&mut self, st: &mut AppState) {
        let Some(c) = self.coach.as_mut() else { return };
        c.progress.done = true;
        if c.progress.finished <= 0.0 {
            c.progress.finished = epoch_now();
        }
        c.dirty = true;
        if c.saved_lick || !matches!(c.riff.kind, Kind::Ear | Kind::Learn) {
            return;
        }
        c.saved_lick = true;
        let (a, b) = (c.riff.start, c.riff.end);
        let name = format!("{} · {}", c.lesson.label(), c.riff.name);
        let mut lick = self.lick_for(a, b);
        lick.name = name;
        lick.canonical = st
            .row_of(&self.entry)
            .map(|g| st.groups[g].canonical.clone())
            .unwrap_or_default();
        st.userdata.add_lick(lick);
    }

    /// Write the progress (and the time spent since the last write) to userdata.json.
    fn coach_save(&mut self, st: &mut AppState) {
        let Some(c) = self.coach.as_mut() else { return };
        c.progress.seconds += c.since.elapsed().as_secs_f64();
        c.since = Instant::now();
        c.progress.step = c.step.key();
        st.userdata
            .set_riff_progress(&c.lesson.id, &c.riff.id, &c.progress);
        c.dirty = false;
    }

    /// The player is closing: keep where the item got to.
    pub fn coach_close(&mut self, st: &mut AppState) {
        if self.coach.is_some() {
            self.coach_save(st);
        }
    }

    /// Plucks for the coach to read (every frame, with the guitar's onsets): while finding notes, running the scale
    /// or playing over the song.
    pub(super) fn coach_plucks(&mut self, onsets: &[qrs_audio::rt::Onset], rate: u32) {
        let page_listens = self.page_listens();
        if let Some(c) = self.coach.as_mut()
            && !c.tuner
            && (matches!(c.step, Step::First | Step::Notes | Step::Run | Step::Jam) || page_listens)
        {
            for o in onsets {
                c.reader.push(o.index, rate);
            }
        }
    }

    /// The notes of the plucks read so far, each to the step listening.
    pub(super) fn coach_listen(&mut self, src: &dyn qrs_analysis::SampleSource) {
        let heard = match self.coach.as_mut() {
            Some(c) if c.reader.waiting() => c.reader.poll(src),
            _ => return,
        };
        let step = self.coach.as_ref().map(|c| c.step);
        let act = self.page().map(|p| p.act);
        for (_, midi) in heard {
            match (step, act) {
                (Some(Step::Run), _) | (Some(Step::Page(_)), Some(Activity::Run)) => {
                    self.coach_run_note(midi)
                }
                (Some(Step::Jam), _) | (Some(Step::Page(_)), Some(Activity::Jam)) => {
                    self.coach_jam_note(midi)
                }
                (Some(Step::Page(_)), Some(Activity::Find)) => self.page_find_note(midi),
                (Some(Step::Page(_)), _) => {}
                _ => self.coach_heard(midi),
            }
        }
    }

    /// One pluck's note (None: it couldn't be read) against the note being looked for, or the next of a run-through
    /// of the notes already found.
    fn coach_heard(&mut self, midi: Option<f64>) {
        let Some(c) = self.coach.as_mut() else { return };
        let Some(k) = c.looking_for() else { return };
        let tg = c.phrase[k].clone();
        let Some(m) = midi else {
            c.unclear = Some(Instant::now());
            return;
        };
        c.unclear = None;
        let v = match hear_pluck(&c.phrase, k, c.run, m) {
            Heard::Through(r) => {
                c.run = r;
                c.through = Some(Instant::now());
                c.said = None;
                return;
            }
            Heard::Judged(v) => v,
        };
        c.run = 0;
        c.through = None;
        c.said = Some((m, v, Instant::now()));
        c.dirty = true;
        match v {
            Verdict::Match => {
                c.progress.found += 1;
                c.hints = 0;
                if c.step == Step::First {
                    c.step = Step::Notes;
                }
                if !c.all_found() {
                    c.hear_at = Some(Instant::now() + Duration::from_secs_f32(NEXT_AFTER));
                }
                self.reveal_note(tg.t, tg.string);
            }
            Verdict::Octave(_) => c.progress.octave += 1,
            Verdict::Off(_) => c.progress.wrong += 1,
        }
    }

    /// Up and down: one pluck's note against the note of the scale due.
    fn coach_run_note(&mut self, midi: Option<f64>) {
        let Some(c) = self.coach.as_mut() else { return };
        let Some(m) = midi else {
            c.unclear = Some(Instant::now());
            return;
        };
        if c.run_seq.is_empty() {
            return;
        }
        c.unclear = None;
        let h = run_pluck(&c.run_seq, c.run_pos, m);
        c.run_said = Some((m, h, Instant::now()));
        match h {
            RunHeard::Next => c.run_pos += 1,
            RunHeard::Again => {}
            RunHeard::Over => {
                c.run_pos = 1;
                c.run_wrong = false;
            }
            RunHeard::Wrong => c.run_wrong = true,
            RunHeard::Done => {
                c.progress.runs += 1;
                c.page_runs += 1;
                if !c.run_wrong {
                    c.progress.clean += 1;
                }
                let clean = !c.run_wrong;
                c.run_pos = 0;
                c.run_wrong = false;
                c.dirty = true;
                let (runs, cl) = (c.progress.runs, c.progress.clean);
                self.say(
                    if clean {
                        "Up and down, clean!  ·  "
                    } else {
                        "Up and down  ·  "
                    },
                    format!("{runs} run{}, {cl} clean", if runs == 1 { "" } else { "s" }),
                );
            }
        }
    }

    /// Over the song: one pluck's note, on the neck and counted in the scale or out.
    fn coach_jam_note(&mut self, midi: Option<f64>) {
        let Some(c) = self.coach.as_mut() else { return };
        let Some(m) = midi else { return };
        let m = m.round() as i32;
        c.jam.push((m, Instant::now()));
        if c.jam.len() > JAM_KEEP {
            c.jam.remove(0);
        }
        c.jam_all += 1;
        if in_scale(c.pcs, m) {
            c.jam_in += 1;
        }
    }

    /// Every frame: the tuner, passes heard, the song up to the next note when it's due, progress written.
    pub(super) fn coach_tick(&mut self, st: &mut AppState) {
        let tuner = self.tuner_midi;
        let passes = self.loop_passes;
        // (the Tuner tile's strings: a bass's 4 or 5, P37.6a)
        let opens = self.tuner_targets().1;
        let mut hear = false;
        let Some(c) = self.coach.as_mut() else { return };
        if c.tuner {
            if c.tuned.len() != opens.len() {
                c.tuned = vec![false; opens.len()];
            }
            // the open string nearest what's heard, held in tune for a moment
            match near_open(tuner, &opens) {
                Some((s, cents)) if cents.abs() <= IN_TUNE => match c.tune_hold {
                    Some((hs, since)) if hs == s => {
                        if since.elapsed().as_secs_f32() >= TUNE_HOLD
                            && let Some(t_) = c.tuned.get_mut(s)
                            && !*t_
                        {
                            *t_ = true;
                        }
                    }
                    _ => c.tune_hold = Some((s, Instant::now())),
                },
                _ => c.tune_hold = None,
            }
        } else if c.step == Step::Listen {
            let n = passes.saturating_sub(c.listen_base);
            if n > c.progress.listens {
                c.progress.listens = n;
                c.dirty = true;
            }
        }
        c.jam.retain(|j| j.1.elapsed().as_secs_f32() < JAM_FOR);
        if c.hear_at.is_some_and(|at| Instant::now() >= at) {
            c.hear_at = None;
            hear = true;
        }
        let dirty = c.dirty;
        if hear {
            self.coach_hear();
        }
        if dirty {
            self.coach_save(st);
        }
    }

    /// A loop pass ended (`acc`: its accuracy, None with no guitar to judge). Play along: the best kept, and a pass
    /// at the item's mark moves up a speed, or at full speed finishes it. Practise and Record: practise.rs.
    pub(super) fn coach_pass(&mut self, st: &mut AppState, acc: Option<f64>) {
        let Some(c) = self.coach.as_mut() else { return };
        match c.step {
            Step::Practise => return self.practise_pass(acc),
            Step::Record => return self.take_pass(st, acc),
            Step::Play => {}
            _ => return,
        }
        c.last_pass = Some(acc);
        let Some(acc) = acc else { return };
        let speed = c.speed();
        c.progress.record(speed, acc);
        c.dirty = true;
        match after_pass(c.speed_i, acc, c.riff.pass) {
            Some(i) if i == c.speed_i => {}
            Some(i) => {
                c.speed_i = i;
                let next = c.speed();
                self.coach_speed(next);
                self.say(
                    format!("Loop pass: {}%  ·  ", pct(acc)),
                    format!("up to {}%", pct(next)),
                );
            }
            None => {
                let last = c.next_step().unwrap_or(Step::Check);
                let check = last == Step::Check;
                self.coach_enter(st, last);
                self.say_for(
                    format!(
                        "Loop pass: {}%  ·  {} learned!  ",
                        pct(acc),
                        if check { "riff" } else { "it's" }
                    ),
                    if check {
                        "Here's the tab"
                    } else {
                        "Saved as a lick"
                    },
                    3.0,
                );
            }
        }
    }

    /// Play the song up to the note being looked for (Justin's "stop the track right after the note"), slowed when
    /// asked; with no song audio, the note on the synth guitar.
    fn coach_hear(&mut self) {
        let Some(c) = self.coach.as_ref() else { return };
        let k = (c.progress.found as usize).min(c.phrase.len().saturating_sub(1));
        let Some((a, b)) = hear_span(&c.phrase, k) else {
            return;
        };
        let speed = if c.slow { SLOW } else { 1.0 };
        if !self.track_ok() {
            self.coach_note();
            return;
        }
        self.countdown_until = None;
        if self.transport.playing() {
            self.transport.pause();
        }
        if let Some(song) = self.transport.song().cloned() {
            self.synth
                .start_track(song, a, b, self.offset, speed, self.mixer.pitch as f32, false);
        }
    }

    /// Notes on the synth guitar: (string, fret) one after another from chart time `t0`, `gap` seconds apart.
    pub(super) fn coach_synth(&mut self, notes: &[(i32, i32)], t0: f64, gap: f64, ring: f64) {
        let Some(e) = self.engine().cloned() else {
            self.say("The synth needs the audio engine", "");
            return;
        };
        self.countdown_until = None;
        if self.transport.playing() {
            self.transport.pause();
        }
        let notes: Vec<SynthNote> = notes
            .iter()
            .enumerate()
            .map(|(i, &(string, fret))| SynthNote {
                t: t0 + i as f64 * gap,
                string,
                fret,
                sus: ring,
                tech: Techs::NONE,
                slide_to: -1,
                bend: 0.0,
                in_chord: false,
            })
            .collect();
        let end = t0 + notes.len().saturating_sub(1) as f64 * gap + ring + 0.4;
        let tone = qrs_audio::synth::synth_tone(&e.tone(), self.chart.arr.is_bass);
        let place: Arc<dyn Fn(f64) -> f64 + Send + Sync> = Arc::new(|x| x);
        self.synth.start(
            notes,
            self.sounding_opens(),
            t0 - 0.02,
            end,
            1.0,
            self.chart.arr.is_bass,
            false,
            tone,
            e.rate(),
            place,
        );
    }

    /// The note being looked for alone, on the synth guitar.
    fn coach_note(&mut self) {
        let Some(c) = self.coach.as_ref() else { return };
        let k = (c.progress.found as usize).min(c.phrase.len().saturating_sub(1));
        let Some(tg) = c.phrase.get(k).cloned() else {
            return;
        };
        self.coach_synth(&[(tg.string, tg.fret)], tg.t, 0.0, 0.9);
    }

    /// The scale's shape up and down on the synth guitar.
    fn coach_hear_run(&mut self) {
        let Some(sc) = self.coach.as_ref().and_then(|c| c.riff.scale.clone()) else {
            return;
        };
        let mut notes = sc.shape.clone();
        notes.extend(sc.shape.iter().rev().skip(1));
        let t0 = self.now();
        self.coach_synth(&notes, t0, RUN_NOTE, 0.5);
    }

    /// The next hint for the note being looked for.
    fn coach_hint(&mut self) {
        let total = self.coach_hints().len();
        if let Some(c) = self.coach.as_mut()
            && c.looking_for().is_some()
            && c.hints < total
        {
            c.hints += 1;
            c.progress.hints += 1;
            c.dirty = true;
        }
    }

    /// Every hint there is for the note being looked for.
    fn coach_hints(&self) -> Vec<String> {
        let Some(c) = self.coach.as_ref() else {
            return Vec::new();
        };
        let Some(k) = c.looking_for() else {
            return Vec::new();
        };
        let tg = &c.phrase[k];
        let prev = k.checked_sub(1).map(|j| &c.phrase[j]);
        let given = c.riff.string_hint && c.strings().len() == 1;
        hints_for(tg, prev, given, &self.string_label(tg.string))
    }

    /// The riff's strings in words: "the G string", "the A and D strings".
    fn coach_strings(&self) -> String {
        let Some(c) = self.coach.as_ref() else {
            return String::new();
        };
        let names: Vec<String> = c.strings().iter().map(|&s| self.string_name(s)).collect();
        match names.as_slice() {
            [] => String::new(),
            [one] => format!("the {one} string"),
            [rest @ .., last] => format!("the {} and {last} strings", rest.join(", ")),
        }
    }

    /// A string's short name ("G").
    fn string_name(&self, s: i32) -> String {
        self.string_label(s)
            .split_whitespace()
            .next()
            .unwrap_or("?")
            .to_string()
    }

    /// A coach action (its buttons and keys, `coach:<what>`).
    pub(super) fn coach_act(&mut self, st: &mut AppState, what: &str) {
        let Some(c) = self.coach.as_ref() else { return };
        let step = c.step;
        match what {
            "next" => {
                if c.tuner {
                    self.coach_tuner();
                    return;
                }
                let next = match step {
                    Step::First | Step::Notes if c.all_found() || c.phrase.is_empty() => Some(Step::Play),
                    Step::First | Step::Notes => None,
                    Step::Record => {
                        self.take_key(st);
                        None
                    }
                    Step::Page(i) if i + 1 >= c.steps.len() => {
                        // the last page: the item's done, on to the next
                        let name = c.riff.name.clone();
                        self.coach_finish(st);
                        self.say_for("Done  ·  ", name, 3.0);
                        self.coach_act(st, "next_riff");
                        None
                    }
                    s if s.last() => {
                        self.coach_act(st, "next_riff");
                        None
                    }
                    _ => c.next_step(),
                };
                if let Some(s) = next {
                    self.coach_enter(st, s);
                } else if matches!(step, Step::First | Step::Notes) {
                    self.say("Find this note first  ·  ", "R plays it again, ? gives a hint");
                }
            }
            "tuner" => self.coach_tuner(),
            "back" => match step {
                Step::Page(i) if i > 0 => self.coach_enter(st, Step::Page(i - 1)),
                Step::Page(_) => self.say("This is the first page", ""),
                Step::Record if !c.take.going() => self.coach_enter(st, Step::Practise),
                _ => {}
            },
            "practise" => self.coach_enter(st, Step::Practise),
            "listen" => self.take_listen(),
            "nbn" => self.practise_nbn(),
            "hear" => match step {
                Step::First | Step::Notes => self.coach_hear(),
                Step::See | Step::Run => self.coach_hear_run(),
                Step::Page(_) => self.page_hear(),
                _ => {}
            },
            "note" => self.coach_note(),
            "slow" => {
                if let Some(c) = self.coach.as_mut() {
                    c.slow = !c.slow;
                    let slow = c.slow;
                    self.say(
                        if slow {
                            "Hear it at 70%"
                        } else {
                            "Hear it at full speed"
                        },
                        "",
                    );
                    self.coach_hear();
                }
            }
            "slower" => self.coach_notch(1),
            "faster" => self.coach_notch(-1),
            "hint" => self.coach_hint(),
            "run_over" => {
                if let Some(c) = self.coach.as_mut() {
                    c.run_pos = 0;
                    c.run_wrong = false;
                    c.run_said = None;
                }
            }
            "restart" => {
                let (lesson, riff) = (c.lesson.clone(), c.riff.id.clone());
                let first = c.steps[0];
                st.userdata.forget_riff(&lesson.id, &riff);
                self.revealed = vec![false; self.chart.glyphs.len()];
                if let Some(c) = self.coach.as_mut() {
                    c.progress = RiffProgress {
                        started: epoch_now(),
                        ..Default::default()
                    };
                    c.speed_i = 0;
                    c.saved_lick = false;
                }
                self.coach_enter(st, first);
                self.say("Starting it over", "");
            }
            "next_riff" => {
                let next = c.lesson.riffs.get(c.index + 1).cloned();
                let lesson = c.lesson.clone();
                self.coach_save(st);
                match next {
                    // the same song and part (a song broken down): on to it here, without opening the song again
                    Some(r)
                        if r.is_song(&self.entry.artist, &self.entry.display_title())
                            && r.part_of(&self.entry.arrangements)
                                .is_some_and(|i| self.entry.arrangements[i].key == self.part_key) =>
                    {
                        self.start_lesson(st, lesson, &r.id);
                    }
                    Some(r) => self.reqs.push(PlayerReq::Riff {
                        lesson: lesson.id.clone(),
                        riff: r.id,
                    }),
                    None => self.reqs.push(PlayerReq::Lessons),
                }
            }
            "lessons" => {
                self.coach_save(st);
                self.reqs.push(PlayerReq::Lessons);
            }
            "show:lesson" => {
                if let Some(c) = self.coach.as_mut() {
                    c.show = true;
                }
            }
            "show:tiles" | "show:controls" => {
                if let Some(c) = self.coach.as_mut() {
                    c.show = false;
                }
                self.show_controls = what == "show:controls";
                self.tile_drag = None;
            }
            _ => {
                if let Some(i) = what.strip_prefix("goto:").and_then(|s| s.parse::<usize>().ok())
                    && let Some(&s) = c.steps.get(i)
                {
                    self.coach_enter(st, s);
                }
            }
        }
    }
}

/// The open string nearest a pitch (MIDI) and how far off it is (cents), when it's within 60 cents of one.
fn near_open(m: Option<f64>, opens: &[i32]) -> Option<(usize, f64)> {
    let m = m?;
    let (s, o) = opens
        .iter()
        .enumerate()
        .min_by(|a, b| (m - *a.1 as f64).abs().total_cmp(&(m - *b.1 as f64).abs()))?;
    let cents = (m - *o as f64) * 100.0;
    (cents.abs() < 60.0).then_some((s, cents))
}

/// For a log line: the lesson and item the coach is on.
fn st_lesson_id(p: &PlayerView) -> String {
    p.coach
        .as_ref()
        .map(|c| format!("{} / {}", c.lesson.id, c.riff.id))
        .unwrap_or_default()
}

/// Screenshot mode: a scale for the shot's stretch (name, root, shape): the minor pentatonic's first box on the
/// stretch's first note, the root's fret on the low string between 3 and 14.
fn shot_scale(chart: &Chart, a: f64, b: f64) -> (String, String, Vec<(i32, i32)>) {
    let i0 = chart.gtimes.partition_point(|&t| t < a);
    let first = chart.glyphs[i0..]
        .iter()
        .take_while(|g| g.t < b)
        .find(|g| g.fret >= 0)
        .and_then(|g| chart.opens.get(g.string as usize).map(|o| o + g.fret))
        .unwrap_or(45);
    let low = chart.opens.first().copied().unwrap_or(40);
    let mut f = (first - low).rem_euclid(12);
    if f < 3 {
        f += 12;
    }
    let pc = (low + f).rem_euclid(12);
    let off: [(i32, i32); 12] = [
        (0, 0),
        (0, 3),
        (1, 0),
        (1, 2),
        (2, 0),
        (2, 2),
        (3, 0),
        (3, 2),
        (4, 0),
        (4, 3),
        (5, 0),
        (5, 3),
    ];
    let strings = chart.opens.len() as i32;
    let shape = off
        .iter()
        .filter(|o| o.0 < strings)
        .map(|&(s, d)| (s, f + d))
        .collect();
    (
        format!("{} minor pentatonic", pc_name(pc)),
        pc_name(pc).to_string(),
        shape,
    )
}

/// A coach button's place, label, key and look, and the action it pushes (`coach:<act>`).
struct Btn<'a> {
    r: Rect,
    id: &'a str,
    label: &'a str,
    key: &'a str,
    primary: bool,
    active: bool,
    act: &'a str,
}

impl PlayerView {
    // -- drawing ------------------------------------------------------------------------------------------------

    /// The coach in the console: which item of which lesson with its steps (a click goes to one) and the Tuner key,
    /// then the step's card (what to do, what's being heard, its keys), or the tuner.
    pub(super) fn draw_coach(&mut self, ui: &Ui, area: Rect) {
        let Some(c) = self.coach.as_ref() else { return };
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        let mut acts: Vec<String> = Vec::new();
        let pad = px(16.0);
        // the header: lesson, item, song, the Tuner key and the steps
        let head = Rect::from_min_size(area.min, vec2(area.width(), px(128.0)));
        paint::display(&p, head, Some(18.0), 6.0);
        let x = head.min.x + pad;
        let w = head.width() - 2.0 * pad;
        let tw_ = button_width(&ctx, "Tuner", "U", 12.0, false) + px(16.0); // (and its light, when it's up)
        let tuner_r = Rect::from_min_size(
            pos2(head.max.x - pad - tw_, head.min.y + px(12.0)),
            vec2(tw_, px(30.0)),
        );
        if button(
            ui,
            tuner_r,
            "coach-tuner",
            "Tuner",
            "U",
            ButtonStyle {
                active: c.tuner,
                size: 12.0,
                r: Some(10.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            acts.push("coach:tuner".into());
        }
        let hw = tuner_r.min.x - px(10.0) - x;
        let kind = match c.riff.kind {
            Kind::Ear => "BY EAR",
            Kind::Learn => "LEARN",
            Kind::Scale => "SCALE",
            Kind::Theory => "THEORY",
        };
        caps(
            &p,
            &format!(
                "{}  ·  {} OF {}  ·  {kind}",
                c.lesson.label().to_uppercase(),
                c.index + 1,
                c.lesson.riffs.len()
            ),
            pos2(x, head.min.y + px(14.0)),
            11.0,
            Align2::LEFT_TOP,
            Some(hw),
        );
        txt(
            &p,
            &c.riff.title,
            pos2(x, head.min.y + px(32.0)),
            18.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            Some(hw),
        );
        txt(
            &p,
            &format!("{}  ·  {}", c.riff.name, c.riff.part),
            pos2(x, head.min.y + px(58.0)),
            12.0,
            th.caption,
            600,
            Align2::LEFT_TOP,
            Some(w),
        );
        let cur = c.step_no();
        let n_steps = c.steps.len();
        let gap = px(6.0);
        let sw = (w - (n_steps - 1) as f32 * gap) / n_steps as f32;
        let ty = head.min.y + px(86.0);
        let mut tip = None;
        for (i, s) in c.steps.iter().enumerate() {
            let r = Rect::from_min_size(pos2(x + i as f32 * (sw + gap), ty), vec2(sw, px(28.0)));
            let resp = interact(ui, r, ("coach-step", i), Sense::click());
            if i == cur {
                paint::raised(&p, r, Some(9.0), 2.0, Fill::Base, 0.0);
                paint::ring(&p, r, Some(9.0), 1.5, th.accent, false);
            } else {
                paint::inset(&p, r, Some(9.0), 2.0, Fill::Base);
            }
            if i < cur {
                icons::icon(&p, Icon::Check, r.center(), 14.0, th.hit);
            } else {
                txt(
                    &p,
                    &(i + 1).to_string(),
                    r.center(),
                    13.0,
                    if i == cur { th.accent_ink } else { th.caption },
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
            if resp.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                tip = Some((
                    c.step_title(*s),
                    String::new(),
                    pos2(r.center().x, r.max.y + px(8.0)),
                    Align2::CENTER_TOP,
                ));
            }
            if resp.clicked() {
                acts.push(format!("coach:goto:{i}"));
            }
        }
        // the step's card
        let card = Rect::from_min_max(pos2(area.min.x, head.max.y + px(12.0)), area.max);
        paint::display(&p, card, Some(18.0), 6.0);
        let x = card.min.x + pad;
        let w = card.width() - 2.0 * pad;
        let mut y = card.min.y + pad;
        text::caps(
            &p,
            &if c.tuner {
                "TUNER".to_string()
            } else {
                format!(
                    "{} {} OF {n_steps}  ·  {}",
                    if c.riff.kind == Kind::Theory {
                        "PAGE"
                    } else {
                        "STEP"
                    },
                    cur + 1,
                    c.step_title(c.step).to_uppercase()
                )
            },
            pos2(x, y),
            11.0,
            th.accent_ink,
            Align2::LEFT_TOP,
            Some(w),
        );
        y += px(22.0);
        let para = |y: &mut f32, s: &str, size: f32, col: Color32, weight: u16| {
            for line in text::wrap(&ctx, s, Style::new(size, weight, col), w) {
                txt(&p, &line, pos2(x, *y), size, col, weight, Align2::LEFT_TOP, None);
                *y += px(size + 6.0);
            }
        };
        para(&mut y, &self.coach_tell(c), 13.0, th.text_2, 500);
        y += px(8.0);
        let take_card = c.step == Step::Record
            && matches!(c.take, super::practise::Take::Done(_))
            && self.take.as_ref().is_some_and(|s| s.report.is_some());
        if !c.tuner
            && matches!(c.step, Step::Listen | Step::Practise | Step::Record)
            && !c.riff.callouts.is_empty()
            && !take_card
        {
            // (room kept for the callout whether one's up or not, so nothing under it moves)
            y = self.draw_callout(&p, c, x, y, w);
        }
        // the step's own part, then its keys from the card's bottom up
        let bh = px(40.0);
        let bottom = card.max.y - pad;
        let half_w = (w - px(8.0)) / 2.0;
        let row = |k: f32| bottom - bh - k * (bh + px(8.0));
        let full = |k: f32| Rect::from_min_size(pos2(x, row(k)), vec2(w, bh));
        let left = |k: f32| Rect::from_min_size(pos2(x, row(k)), vec2(half_w, bh));
        let right = |k: f32| Rect::from_min_size(pos2(x + half_w + px(8.0), row(k)), vec2(half_w, bh));
        let mut btns: Vec<Btn> = Vec::new();
        let b = |r: Rect,
                 id: &'static str,
                 label: &'static str,
                 key: &'static str,
                 primary: bool,
                 act: &'static str| Btn {
            r,
            id,
            label,
            key,
            primary,
            active: false,
            act,
        };
        let next_label;
        if c.tuner {
            self.draw_tune(&p, c, x, y, w);
            btns.push(b(full(0.0), "back", "Back to the lesson", "U", true, "tuner"));
        } else {
            match c.step {
                Step::Listen => {
                    self.draw_listen(&p, c, x, y, w);
                    let heard = self.loop_passes.saturating_sub(c.listen_base);
                    let enough = heard >= c.riff.listens();
                    let label = if c.riff.kind == Kind::Ear {
                        "Find the first note"
                    } else {
                        "Practise it"
                    };
                    btns.push(b(full(0.0), "next", label, "Enter", enough, "next"));
                }
                Step::First | Step::Notes => {
                    let hints = self.coach_hints();
                    let y2 = self.draw_phrase(&p, c, x, y, w);
                    self.draw_verdict(&p, c, x, y2 + px(12.0), w, &hints);
                    if c.all_found() {
                        btns.push(b(
                            full(1.0),
                            "next",
                            "Play it with the record",
                            "Enter",
                            true,
                            "next",
                        ));
                    } else {
                        btns.push(b(left(1.0), "hear", "Hear it", "R", true, "hear"));
                        btns.push(Btn {
                            active: c.slow,
                            ..b(right(1.0), "slow", "Slower", "", false, "slow")
                        });
                    }
                    btns.push(b(left(0.0), "note", "Just the note", "", false, "note"));
                    let more = c.hints < hints.len() && c.looking_for().is_some();
                    let hint = if !more && c.looking_for().is_some() {
                        "No more hints"
                    } else if c.hints == 0 {
                        "Hint"
                    } else {
                        "Another hint"
                    };
                    btns.push(b(right(0.0), "hint", hint, "?", false, "hint"));
                }
                Step::Play => {
                    let yy = self.draw_ladder(&p, c, x, y, w);
                    self.draw_how(&p, c, x, yy + px(10.0), w, row(1.0) - px(10.0));
                    let slowest = c.speed_i + 1 >= c.ladder.len();
                    btns.push(b(left(1.0), "slower", "Slower", "Ctrl+=", false, "slower"));
                    btns.push(b(right(1.0), "faster", "Faster", "=", false, "faster"));
                    if slowest {
                        btns[0].label = "Slowest";
                    }
                    if c.speed_i == 0 {
                        btns[1].label = "Full speed";
                    }
                    let skip = if c.riff.kind == Kind::Ear {
                        "Skip to the tab"
                    } else {
                        "Skip: I've got it"
                    };
                    btns.push(b(full(0.0), "skip", skip, "Enter", false, "next"));
                }
                Step::Practise => {
                    let yy = self.draw_ladder(&p, c, x, y, w);
                    let yy = if c.nbn {
                        self.draw_learn(&p, c, x, yy + px(6.0), w)
                    } else {
                        yy
                    };
                    self.draw_how(&p, c, x, yy + px(10.0), w, row(2.0) - px(10.0));
                    let slowest = c.speed_i + 1 >= c.ladder.len();
                    btns.push(b(left(2.0), "slower", "Slower", "Ctrl+=", false, "slower"));
                    btns.push(b(right(2.0), "faster", "Faster", "=", false, "faster"));
                    if slowest {
                        btns[0].label = "Slowest";
                    }
                    if c.speed_i == 0 {
                        btns[1].label = "Full speed";
                    }
                    btns.push(Btn {
                        active: c.nbn,
                        ..b(full(1.0), "nbn", "Note by note", "W", false, "nbn")
                    });
                    let ready = c.progress.best_at(1.0).is_some_and(|a| a >= c.riff.pass);
                    btns.push(b(full(0.0), "next", "Ready to record", "Enter", ready, "next"));
                }
                Step::Record => {
                    let (yy, bar) = self.draw_take(ui, &p, c, x, y, w, row(2.0) - px(10.0));
                    if let Some(t0) = bar {
                        acts.push(format!("take:bar:{t0}"));
                    }
                    if !take_card {
                        self.draw_how(&p, c, x, yy + px(10.0), w, row(2.0) - px(10.0));
                    }
                    if !c.take.going() {
                        btns.push(b(left(2.0), "slower", "Slower", "Ctrl+=", false, "slower"));
                        btns.push(b(right(2.0), "faster", "Faster", "=", false, "faster"));
                        if c.speed_i + 1 >= c.ladder.len() {
                            btns[0].label = "Slowest";
                        }
                        if c.speed_i == 0 {
                            btns[1].label = "Full speed";
                        }
                        if take_card {
                            let listening = self.take.as_ref().is_some_and(|s| s.listening());
                            btns.push(b(left(1.0), "back", "Practise", "Backspace", false, "back"));
                            btns.push(b(
                                right(1.0),
                                "listen",
                                if listening { "Stop" } else { "Listen back" },
                                "B",
                                false,
                                "listen",
                            ));
                        } else {
                            btns.push(b(
                                full(1.0),
                                "back",
                                "Back to practise",
                                "Backspace",
                                false,
                                "back",
                            ));
                        }
                    }
                    let (label, primary) = c.take.key_label();
                    btns.push(b(full(0.0), "take", label, "Enter", primary, "next"));
                }
                Step::Page(_) => {
                    let last = cur + 1 >= n_steps;
                    let yy = self.draw_page_card(&p, c, x, y, w);
                    let _ = yy;
                    if self.page().is_some_and(|pg| pg.act == Activity::Run) {
                        btns.push(b(full(2.0), "over", "From the bottom", "", false, "run_over"));
                    }
                    btns.push(b(left(1.0), "back", "Back", "Backspace", false, "back"));
                    btns.push(b(right(1.0), "hear", "Hear it", "R", false, "hear"));
                    btns.push(b(
                        full(0.0),
                        "next",
                        if last { "Finish" } else { "Next page" },
                        "Enter",
                        self.page_ready(c),
                        "next",
                    ));
                }
                Step::See => {
                    let r = self.box_rect(c, x, y, w);
                    self.draw_box(&p, c, r, true);
                    let yy = self.draw_box_legend(&p, c, x, r.max.y + px(8.0), w);
                    self.draw_how(&p, c, x, yy + px(10.0), w, row(1.0) - px(10.0));
                    btns.push(b(full(1.0), "hear", "Hear the shape", "R", false, "hear"));
                    btns.push(b(full(0.0), "next", "Play it up and down", "Enter", true, "next"));
                }
                Step::Run => {
                    let r = self.box_rect(c, x, y, w);
                    self.draw_box(&p, c, r, false);
                    self.draw_run(&p, c, x, r.max.y + px(10.0), w);
                    btns.push(b(left(1.0), "hear", "Hear it", "R", false, "hear"));
                    btns.push(b(right(1.0), "over", "From the bottom", "", false, "run_over"));
                    btns.push(b(
                        full(0.0),
                        "next",
                        "Play over the song",
                        "Enter",
                        c.progress.runs > 0,
                        "next",
                    ));
                }
                Step::Jam => {
                    let r = self.box_rect(c, x, y, w);
                    self.draw_box(&p, c, r, false);
                    self.draw_jam(&p, c, x, r.max.y + px(10.0), w);
                    self.draw_how(&p, c, x, r.max.y + px(100.0), w, row(0.0) - px(10.0));
                    btns.push(b(full(0.0), "next", "Done", "Enter", c.jam_all > 0, "next"));
                }
                Step::Check | Step::Done => {
                    if c.step == Step::Check {
                        self.draw_check(&p, c, x, y, w);
                    } else {
                        self.draw_done(&p, c, x, y, w);
                    }
                    next_label = match c.lesson.riffs.get(c.index + 1) {
                        // (a song broken down: the next part's name, else the next song)
                        Some(r) if r.title == c.riff.title && !r.name.is_empty() => {
                            format!("Next: {}", r.name)
                        }
                        Some(r) => format!("Next: {}", r.title),
                        None => "Back to the lessons".to_string(),
                    };
                    btns.push(b(full(1.0), "next_riff", "", "Enter", true, "next_riff"));
                    btns[0].label = &next_label;
                    btns.push(b(left(0.0), "again", "Start over", "", false, "restart"));
                    btns.push(b(right(0.0), "lessons", "Lessons", "", false, "lessons"));
                }
            }
        }
        for bt in &btns {
            if button(
                ui,
                bt.r,
                ("coach-btn", bt.id.to_string()),
                bt.label,
                bt.key,
                ButtonStyle {
                    primary: bt.primary,
                    active: bt.active,
                    size: 13.0,
                    r: Some(12.0),
                    ..Default::default()
                },
            )
            .clicked()
            {
                acts.push(format!("coach:{}", bt.act));
            }
        }
        self.acts.extend(acts);
        if tip.is_some() {
            self.tip = tip;
        }
    }

    /// What the step's card says to do.
    fn coach_tell(&self, c: &Coach) -> String {
        if c.tuner {
            return "Play each open string and tune it until its box lights up. Back to the lesson when you're done."
                .into();
        }
        let strings = self.coach_strings();
        let n = c.phrase.len();
        let scale = c
            .riff
            .scale
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("The scale");
        match c.step {
            Step::Listen if c.riff.kind == Kind::Ear => format!(
                "Hands off the guitar. Hear the riff {} times and hum along: where does it go up, where down, \
                 which notes are long?",
                c.riff.listens()
            ),
            Step::Listen if c.riff.listens() == 1 => "Hear it once with the tab in front of you. Follow the notes as \
                                                        they go by: which strings, which frets, where does your hand \
                                                        move?"
                .to_string(),
            Step::Listen => format!(
                "Hear it {} times with the tab in front of you. Follow the notes as they go by: which strings, which \
                 frets, where does your hand move?",
                c.riff.listens()
            ),
            Step::First => {
                let wher = if c.riff.string_hint && !strings.is_empty() {
                    format!("It starts on {strings}.")
                } else {
                    "No string given this time: try anywhere.".to_string()
                };
                format!(
                    "Find the first note. {wher} Play a note: the coach says higher or lower until you've got it. \
                     R plays it again."
                )
            }
            Step::Notes if c.all_found() => {
                format!("All {n} notes found by ear. Now play them with the record, at full speed.")
            }
            Step::Notes => "Now the next one. R plays the riff up to the note you're after: is it higher or lower \
                            than the last one? A small step or a jump?"
                .to_string(),
            Step::Play => format!(
                "Play it with the record at full speed: {}% on a pass and it's done. Too fast? Slower, and a good \
                 pass brings the speed back up.",
                pct(c.riff.pass)
            ),
            Step::Practise if c.nbn => "Note by note: the song waits at each note or chord until you play it. Enter \
                                        skips one it won't hear; W turns it off to play it out again."
                .to_string(),
            Step::Practise => "Play it out with the record, as often as you like: every pass is scored. Slower if it \
                               runs away from you, note by note (W) for a tricky spot. Ready? Record a take."
                .to_string(),
            Step::Record => format!(
                "One take of the whole part, after a count-in. {}% at full speed finishes it; a take at a slower \
                 speed is kept, but doesn't.",
                pct(c.riff.pass)
            ),
            Step::Page(_) => self.page_tell(c),
            Step::See => format!(
                "{scale}: the dots are the shape, the gold ones its roots. The rings are the notes of the stretch: \
                 the riff lives inside the box."
            ),
            Step::Run => "Play the shape from its lowest note to its highest and back down, one note at a time. Each \
                          note lights as you play it; the same note fretted somewhere else counts too."
                .to_string(),
            Step::Jam => "The stretch loops with the tab hidden. Play the riff, then make up your own with the shape: \
                          your notes land on the neck, green in the scale, red out of it."
                .to_string(),
            Step::Check => "Here's the tab. Your notes in another place on the neck are just as right; where the \
                            record plays them is often easiest."
                .to_string(),
            Step::Done if c.riff.kind == Kind::Scale => {
                "Now you know where the song's notes live. Come back to the shape whenever you jam.".to_string()
            }
            Step::Done => "It's saved as a lick: loop it any time from Saved licks, or carry on with the next one."
                .to_string(),
        }
    }

    /// Listen: a gauge of the times heard.
    fn draw_listen(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) {
        let th = t();
        // (counted afresh each time the step starts: the ears want it again after a break)
        let heard = self.loop_passes.saturating_sub(c.listen_base);
        let want = c.riff.listens();
        let enough = heard >= want;
        let gc = pos2(x + w / 2.0, y + px(64.0));
        knobs::ring_gauge(
            p,
            gc,
            56.0,
            8.0,
            (heard.min(want) as f32 / want.max(1) as f32).clamp(0.0, 1.0),
            Some(if enough { th.hit } else { th.accent }),
        );
        txt(
            p,
            &format!("{}", heard.min(99)),
            pos2(gc.x, gc.y - px(6.0)),
            34.0,
            th.text,
            800,
            Align2::CENTER_CENTER,
            None,
        );
        txt(
            p,
            &format!("of {want}"),
            pos2(gc.x, gc.y + px(22.0)),
            12.0,
            th.caption,
            700,
            Align2::CENTER_CENTER,
            None,
        );
        let msg = if !self.transport.playing() && self.countdown_until.is_none() {
            "Space plays it again".to_string()
        } else if enough && c.riff.kind == Kind::Ear {
            "Can you hum it? Then you're ready.".to_string()
        } else if enough {
            "Seen where it goes? Then you're ready.".to_string()
        } else {
            "times heard".to_string()
        };
        txt(
            p,
            &msg,
            pos2(gc.x, gc.y + px(78.0)),
            13.0,
            if enough { th.hit } else { th.caption },
            700,
            Align2::CENTER_TOP,
            Some(w),
        );
    }

    /// The tuner: a box for each open string (lit once heard in tune), and a needle for the one you're playing.
    /// The strings are the Tuner tile's (`tuner_targets`, P37.6a): a bass's four or five, a guitar's part's.
    fn draw_tune(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) {
        let th = t();
        let opens = self.tuner_targets().1;
        let n = opens.len().max(1);
        let gap = px(6.0);
        let bw = (w - (n - 1) as f32 * gap) / n as f32;
        let names: Vec<String> = if opens == self.chart.opens {
            (0..n).map(|s| self.string_name(s as i32)).collect()
        } else {
            opens
                .iter()
                .map(|&m| qrs_analysis::pitch::note_label(m as f64))
                .collect()
        };
        for s in 0..n {
            let r = Rect::from_min_size(pos2(x + s as f32 * (bw + gap), y), vec2(bw, px(46.0)));
            let ok = c.tuned.get(s).copied().unwrap_or(false);
            paint::inset(p, r, Some(10.0), 2.0, Fill::Base);
            if ok {
                paint::ring(p, r, Some(10.0), 1.5, th.hit, false);
            }
            txt(
                p,
                &names[s],
                pos2(r.center().x, r.min.y + px(14.0)),
                15.0,
                if ok { th.hit } else { th.text },
                800,
                Align2::CENTER_CENTER,
                None,
            );
            if ok {
                icons::icon(
                    p,
                    Icon::Check,
                    pos2(r.center().x, r.max.y - px(12.0)),
                    13.0,
                    th.hit,
                );
            }
        }
        // the needle: the nearest open string, cents off
        let my = y + px(46.0) + px(22.0);
        let track = Rect::from_min_size(pos2(x, my), vec2(w, px(14.0)));
        paint::inset(p, track, None, 2.0, Fill::Base);
        let mid = track.center().x;
        p.rect_filled(
            Rect::from_center_size(pos2(mid, track.center().y), vec2(px(2.0), px(14.0))),
            0.0,
            th.tick,
        );
        let label = match near_open(self.tuner_midi, &opens) {
            Some((s, cents)) => {
                let k = (cents / 50.0).clamp(-1.0, 1.0) as f32;
                let col = if cents.abs() <= IN_TUNE { th.hit } else { th.late };
                paint::led(
                    p,
                    pos2(mid + k * (w / 2.0 - px(8.0)), track.center().y),
                    12.0,
                    col,
                );
                format!(
                    "{}  {:+.0}¢  {}",
                    names.get(s).map(String::as_str).unwrap_or("?"),
                    cents,
                    if cents.abs() <= IN_TUNE {
                        "in tune"
                    } else if cents > 0.0 {
                        "tune down a little"
                    } else {
                        "tune up a little"
                    }
                )
            }
            None if !self.has_input() => "No input: Settings (F2) > Audio input / output".into(),
            None => "Play an open string".into(),
        };
        txt(
            p,
            &label,
            pos2(mid, my + px(24.0)),
            13.0,
            th.text_2,
            700,
            Align2::CENTER_TOP,
            Some(w),
        );
        if c.tuned.iter().all(|t_| *t_) {
            txt(
                p,
                "All in tune",
                pos2(mid, my + px(48.0)),
                14.0,
                th.hit,
                800,
                Align2::CENTER_TOP,
                None,
            );
        }
    }

    /// Play along: the speeds on offer, slowest to fastest (the one you're at lit, the ones passed ticked, each
    /// with its best pass), and the last pass. Returns the y under it.
    fn draw_ladder(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let n = c.ladder.len().max(1);
        let gap = px(8.0);
        let bw = (w - (n - 1) as f32 * gap) / n as f32;
        for (i, s) in c.ladder.iter().enumerate() {
            let col_i = n - 1 - i; // fastest at the right
            let r = Rect::from_min_size(pos2(x + col_i as f32 * (bw + gap), y), vec2(bw, px(64.0)));
            let best = c.progress.best_at(*s);
            let passed = best.is_some_and(|a| a >= c.riff.pass);
            if i == c.speed_i {
                paint::raised(p, r, Some(12.0), 3.0, Fill::Base, 0.0);
                paint::ring(p, r, Some(12.0), 1.5, th.accent, false);
            } else {
                paint::inset(p, r, Some(12.0), 2.0, Fill::Base);
            }
            txt(
                p,
                &format!("{}%", pct(*s)),
                pos2(r.center().x, r.min.y + px(20.0)),
                17.0,
                if i == c.speed_i { th.accent_ink } else { th.text },
                800,
                Align2::CENTER_CENTER,
                None,
            );
            let (t_, col) = match best {
                Some(a) => (
                    format!("best {}%", pct(a)),
                    if passed { th.hit } else { th.text_2 },
                ),
                None => ("–".into(), th.caption),
            };
            if passed {
                icons::icon(
                    p,
                    Icon::Check,
                    pos2(r.max.x - px(12.0), r.min.y + px(12.0)),
                    12.0,
                    th.hit,
                );
            }
            txt(
                p,
                &t_,
                pos2(r.center().x, r.max.y - px(16.0)),
                12.0,
                col,
                700,
                Align2::CENTER_CENTER,
                Some(bw - px(4.0)),
            );
        }
        let ly = y + px(64.0) + px(16.0);
        let top = c.speed_i == 0;
        let practise = c.step == Step::Practise;
        let (msg, col) = match c.last_pass {
            Some(Some(a)) if practise && a >= c.riff.pass && top => {
                (format!("Last pass: {}%  ·  ready to record", pct(a)), th.hit)
            }
            Some(Some(a)) if practise && a >= c.riff.pass => {
                (format!("Last pass: {}%  ·  try it faster", pct(a)), th.hit)
            }
            Some(Some(a)) if practise => (format!("Last pass: {}%", pct(a)), th.text_2),
            None if practise && c.nbn && self.countdown_until.is_none() && self.transport.playing() => {
                ("Note by note: a practice pass, not scored".into(), th.caption)
            }
            Some(Some(a)) if a >= c.riff.pass && top => (format!("Last pass: {}%  ·  done", pct(a)), th.hit),
            Some(Some(a)) if a >= c.riff.pass => (format!("Last pass: {}%  ·  up you go", pct(a)), th.hit),
            Some(Some(a)) => (
                format!(
                    "Last pass: {}%  ·  {}% {}",
                    pct(a),
                    pct(c.riff.pass),
                    if top { "and it's done" } else { "moves you up" }
                ),
                th.text_2,
            ),
            Some(None) if !self.has_input() => (
                "No input to judge: Settings (F2) > Audio input / output, or skip".into(),
                th.miss,
            ),
            Some(None) => ("Play every note of it on a pass".into(), th.text_2),
            None if self.countdown_until.is_some() => ("Get ready…".into(), th.text_2),
            None if !self.transport.playing() => ("Space plays the loop".into(), th.caption),
            None => ("Playing: every pass is scored".into(), th.caption),
        };
        let ctx = p.ctx().clone();
        let mut yy = ly;
        for line in text::wrap(&ctx, &msg, Style::new(13.0, 700, col), w) {
            txt(
                p,
                &line,
                pos2(x + w / 2.0, yy),
                13.0,
                col,
                700,
                Align2::CENTER_TOP,
                None,
            );
            yy += px(19.0);
        }
        yy
    }

    /// Practise, note by note: the note or chord the song is waiting at (its frets and strings). Returns the y under
    /// it.
    fn draw_learn(&self, p: &egui::Painter, _c: &Coach, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let r = Rect::from_min_size(pos2(x, y), vec2(w, px(84.0)));
        paint::inset(p, r, Some(14.0), 2.0, Fill::Base);
        let group = self
            .waiting
            .and_then(|wt| self.scorer.groups.get(wt.gi).map(|g| (wt, g)));
        let (head, sub, col) = match group {
            Some((wt, g)) => {
                let frets: Vec<(i32, i32)> = g
                    .strings
                    .iter()
                    .zip(&g.midis)
                    .map(|(&s, &m)| (s, m - self.chart.opens.get(s as usize).copied().unwrap_or(m)))
                    .collect();
                let chord = self
                    .chart
                    .chords
                    .iter()
                    .find(|ch| (ch.time - g.t).abs() < 0.02)
                    .map(|ch| ch.name.trim().to_string())
                    .filter(|n| !n.is_empty());
                let head = match (frets.as_slice(), chord) {
                    (_, Some(name)) => name,
                    ([(_, f)], None) if *f == 0 => "Open".to_string(),
                    ([(_, f)], None) => format!("Fret {f}"),
                    _ if g.muted => "Muted".to_string(),
                    _ => format!("{} notes", frets.len()),
                };
                let sub = if frets.len() == 1 {
                    format!("{} string", self.string_name(frets[0].0))
                } else {
                    frets
                        .iter()
                        .map(|&(s, f)| format!("{} {f}", self.string_name(s)))
                        .collect::<Vec<_>>()
                        .join("  ·  ")
                };
                let wrong = wt.wrong() > 0.0;
                (head, sub, if wrong { th.miss } else { th.accent_ink })
            }
            None if self.countdown_until.is_some() => ("Get ready…".into(), String::new(), th.text_2),
            None if !self.has_input() => (
                "No input".into(),
                "Settings (F2) > Audio input / output: the song can't wait for you".into(),
                th.miss,
            ),
            None if self.transport.playing() => {
                ("Playing".into(), "in time, so it goes on".into(), th.text_2)
            }
            None => ("Space plays it".into(), String::new(), th.caption),
        };
        caps(
            p,
            if group.is_some() { "YOUR NOTE" } else { "THE SONG" },
            pos2(r.min.x + px(14.0), r.min.y + px(10.0)),
            10.0,
            Align2::LEFT_TOP,
            None,
        );
        txt(
            p,
            &head,
            pos2(r.center().x, r.min.y + px(42.0)),
            22.0,
            col,
            800,
            Align2::CENTER_CENTER,
            Some(r.width() - px(20.0)),
        );
        if !sub.is_empty() {
            txt(
                p,
                &sub,
                pos2(r.center().x, r.max.y - px(14.0)),
                12.0,
                th.text_2,
                700,
                Align2::CENTER_CENTER,
                Some(r.width() - px(20.0)),
            );
        }
        r.max.y + px(4.0)
    }

    /// The scale's box: as wide as the card, a string every 22 points or so (fewer on a bass).
    fn box_rect(&self, _c: &Coach, x: f32, y: f32, w: f32) -> Rect {
        let n = self.chart.n_strings.max(1) as f32;
        Rect::from_min_size(pos2(x, y), vec2(w, px(18.0) + (n - 1.0) * px(22.0) + px(16.0)))
    }

    /// Where a played note goes on the box: where the shape has it, else on a string within the box's frets.
    fn box_spot(&self, c: &Coach, midi: i32, frets: (i32, i32)) -> Option<(i32, i32)> {
        let opens = &self.chart.opens;
        let shape = c.riff.scale.as_ref().map(|s| s.shape.as_slice()).unwrap_or(&[]);
        shape
            .iter()
            .copied()
            .find(|&(s, f)| opens.get(s as usize).is_some_and(|o| o + f == midi))
            .or_else(|| {
                (0..opens.len() as i32).find_map(|s| {
                    let f = midi - opens[s as usize];
                    (frets.0..=frets.1).contains(&f).then_some((s, f))
                })
            })
    }

    /// The scale's box on a stretch of the neck (low string at the bottom, like the tab): the shape's notes as dots
    /// (the roots gold), and the stretch's notes as rings (`see`); running it, the notes played so far lit and the
    /// one due ringed; over the song, the notes you've just played, green in the scale and red out of it.
    fn draw_box(&self, p: &egui::Painter, c: &Coach, r: Rect, see: bool) {
        let th = t();
        let Some(sc) = c.riff.scale.as_ref() else {
            txt(
                p,
                "This item has no scale",
                r.center(),
                13.0,
                th.caption,
                700,
                Align2::CENTER_CENTER,
                None,
            );
            return;
        };
        let opens = &self.chart.opens;
        let n = self.chart.n_strings.max(1) as i32;
        let (lo, hi) = box_frets(&sc.shape, &c.stretch);
        let num_h = px(16.0);
        let board = Rect::from_min_max(r.min, pos2(r.max.x, r.max.y - num_h));
        paint::inset(p, board, Some(10.0), 3.0, Fill::Color(th.neck));
        let nut = lo == 0;
        let nut_w = if nut { px(26.0) } else { px(4.0) }; // (room for the open strings left of the nut)
        let cols = (hi - lo.max(1) + 1).max(1);
        let x0 = board.min.x + nut_w;
        let fw = (board.width() - nut_w - px(4.0)) / cols as f32;
        let m_ = px(9.0);
        let gap = (board.height() - 2.0 * m_) / (n - 1).max(1) as f32;
        let sy = |s: i32| board.min.y + m_ + (n - 1 - s) as f32 * gap;
        let first = lo.max(1);
        let fx = |f: i32| {
            if f == 0 {
                board.min.x + nut_w / 2.0
            } else {
                x0 + ((f - first) as f32 + 0.5) * fw
            }
        };
        // inlays, frets, the nut, strings
        let mid = board.center().y;
        for f in first..=hi {
            let dots = match f % 12 {
                0 => 2,
                3 | 5 | 7 | 9 => 1,
                _ => 0,
            };
            for j in 0..dots {
                let cy = mid + (j as f32 - (dots - 1) as f32 / 2.0) * gap * 2.0;
                paint::dot(p, pos2(fx(f), cy), 3.0, th.inlay);
            }
        }
        let ppp = p.pixels_per_point();
        let lw = (px(1.5) * ppp).round().max(1.0) / ppp;
        for k in 0..=cols {
            let xx = ((x0 + k as f32 * fw) * ppp).round() / ppp;
            p.rect_filled(
                Rect::from_min_max(
                    pos2(xx - lw / 2.0, board.min.y + px(4.0)),
                    pos2(xx + lw / 2.0, board.max.y - px(4.0)),
                ),
                0.0,
                th.fret,
            );
        }
        if nut {
            p.rect_filled(
                Rect::from_min_size(
                    pos2(board.min.x + nut_w - px(4.0), board.min.y + px(4.0)),
                    vec2(px(4.0), board.height() - px(8.0)),
                ),
                px(2.0),
                th.nut,
            );
        }
        let widths = [2.2, 1.9, 1.6, 1.3, 1.1, 1.0];
        for s in 0..n {
            // (whole pixels, at least one: a thinner line at a fractional place doesn't show)
            let thk = (px(widths[(s as usize).min(5)]) * ppp).round().max(1.0) / ppp;
            let top = ((sy(s) - thk / 2.0) * ppp).round() / ppp;
            p.rect_filled(
                Rect::from_min_size(
                    pos2(board.min.x + px(3.0), top),
                    vec2(board.width() - px(6.0), thk),
                ),
                0.0,
                if s < 3 || self.chart.arr.is_bass {
                    th.wound
                } else {
                    th.plain
                },
            );
        }
        for f in first..=hi {
            if f % 2 == 1 || f % 12 == 0 || f == first {
                txt(
                    p,
                    &f.to_string(),
                    pos2(fx(f), board.max.y + px(2.0)),
                    9.0,
                    th.caption,
                    700,
                    Align2::CENTER_TOP,
                    None,
                );
            }
        }
        // the shape: a dot each, the roots gold; running it, the notes played lit and the one due ringed
        let root = sc.root_pc();
        let rad = (gap * 0.36).clamp(px(6.0), px(9.0));
        let up = c.run_seq.len().div_ceil(2);
        let (played, due): (Vec<usize>, Option<usize>) = if c.step == Step::Run && !c.run_seq.is_empty() {
            // the shape's note at each place in the run, the ones played so far this way, and the one due
            let idx = |k: usize| if k < up { k } else { c.run_seq.len() - 1 - k };
            let d = idx(c.run_pos);
            let lit = if c.run_pos < up {
                (0..c.run_pos).collect()
            } else {
                (d + 1..up).collect()
            };
            (lit, Some(d))
        } else {
            (Vec::new(), None)
        };
        let now = p.ctx().input(|i| i.time) as f32;
        for (i, &(s, f)) in sc.shape.iter().enumerate() {
            if s >= n || f < lo || f > hi {
                continue;
            }
            let at = pos2(fx(f), sy(s));
            let is_root = root.is_some_and(|pc| {
                opens
                    .get(s as usize)
                    .is_some_and(|o| (o + f).rem_euclid(12) == pc)
            });
            let lit = played.contains(&i);
            let fill = if lit {
                th.hit
            } else if is_root {
                th.accent
            } else {
                th.key_white
            };
            if lit {
                paint::halo(p, at, 13.0, th.hit, 90);
            }
            p.circle_filled(at, rad, fill);
            if due == Some(i) {
                let pulse = 0.5 + 0.5 * (now * 5.0).sin();
                p.ctx().request_repaint();
                p.circle_stroke(
                    at,
                    rad + px(3.0) + pulse * px(2.0),
                    Stroke::new(px(2.0), th.accent_ink),
                );
            }
        }
        // the stretch's own notes, as rings
        if see {
            for &(s, f) in &c.stretch {
                if s >= n || f < lo || f > hi {
                    continue;
                }
                p.circle_stroke(pos2(fx(f), sy(s)), rad + px(3.5), Stroke::new(px(2.0), th.hit));
            }
        }
        // over the song: your notes, fading
        if c.step == Step::Jam {
            for &(m, at) in &c.jam {
                let Some((s, f)) = self.box_spot(c, m, (lo, hi)) else {
                    continue;
                };
                let k = (1.0 - at.elapsed().as_secs_f32() / JAM_FOR).clamp(0.0, 1.0);
                let col = if in_scale(c.pcs, m) { th.hit } else { th.miss };
                let pos = pos2(fx(f), sy(s));
                p.circle_filled(pos, rad + px(2.0), col.gamma_multiply(0.35 * k));
                p.circle_stroke(pos, rad + px(2.0), Stroke::new(px(2.5), col.gamma_multiply(k)));
            }
        }
    }

    /// Under the box: what its marks are, and the scale's name. Returns the y under it.
    fn draw_box_legend(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let mut xx = x;
        let cy = y + px(8.0);
        let item = |xx: &mut f32, label: &str, draw: &dyn Fn(egui::Pos2)| {
            draw(pos2(*xx + px(6.0), cy));
            let r = txt(
                p,
                label,
                pos2(*xx + px(16.0), cy),
                12.0,
                th.text_2,
                700,
                Align2::LEFT_CENTER,
                None,
            );
            *xx = r.max.x + px(14.0);
        };
        item(&mut xx, "root", &|at| {
            p.circle_filled(at, px(5.0), th.accent);
        });
        item(&mut xx, "scale", &|at| {
            // (an edge: ivory on a light card)
            p.circle_filled(at, px(5.0), th.key_white);
            p.circle_stroke(at, px(5.0), Stroke::new(qrs_ui::units::px_line(1.0), th.caption));
        });
        item(&mut xx, "the stretch's notes", &|at| {
            p.circle_stroke(at, px(5.0), Stroke::new(px(2.0), th.hit));
        });
        let mut yy = y + px(22.0);
        let n_out = c.stretch.len()
            - c.stretch
                .iter()
                .filter(|&&(s, f)| {
                    self.chart
                        .opens
                        .get(s as usize)
                        .is_some_and(|o| in_scale(c.pcs, o + f))
                })
                .count();
        let note = match n_out {
            0 => "Every note of the stretch is in the scale".to_string(),
            1 => "One note of the stretch is outside the scale".to_string(),
            k => format!("{k} notes of the stretch are outside the scale"),
        };
        if !c.stretch.is_empty() && w > 0.0 {
            txt(
                p,
                &note,
                pos2(x, yy),
                12.0,
                th.caption,
                600,
                Align2::LEFT_TOP,
                Some(w),
            );
            yy += px(18.0);
        }
        yy
    }

    /// Up and down: which way, how far, what the last pluck was, and the runs so far.
    pub(super) fn draw_run(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) {
        let th = t();
        let mid = x + w / 2.0;
        let up = c.run_seq.len().div_ceil(2);
        let (dir, k, of) = if c.run_down() {
            ("Down", c.run_pos - up, c.run_seq.len() - up)
        } else {
            ("Up", c.run_pos, up)
        };
        let due = c.run_seq.get(c.run_pos).copied();
        txt(
            p,
            &format!("{dir}: {k} of {of}"),
            pos2(mid, y + px(4.0)),
            20.0,
            th.text,
            800,
            Align2::CENTER_TOP,
            Some(w),
        );
        let fresh = c.run_said.filter(|s| s.2.elapsed().as_secs_f32() < SAID_FOR);
        let unclear = c.unclear.is_some_and(|u| u.elapsed().as_secs_f32() < SAID_FOR);
        let (msg, col) = match (fresh, due) {
            (Some((m, RunHeard::Wrong, _)), Some(d)) => (
                format!(
                    "Not that one: you played {}, next is {}",
                    qrs_analysis::pitch::note_label(m),
                    qrs_analysis::pitch::note_label(d as f64)
                ),
                th.miss,
            ),
            (Some((_, RunHeard::Over, _)), _) => ("From the bottom again".into(), th.text_2),
            (Some((_, RunHeard::Next | RunHeard::Again, _)), Some(d)) => (
                format!("Good  ·  next is {}", qrs_analysis::pitch::note_label(d as f64)),
                th.hit,
            ),
            _ if unclear => ("Didn't catch that one: pick it clearly".into(), th.caption),
            (_, Some(d)) if !self.has_input() => (
                format!(
                    "Next is {}  ·  no input: Settings (F2) > Audio input / output",
                    qrs_analysis::pitch::note_label(d as f64)
                ),
                th.miss,
            ),
            (_, Some(d)) => (
                format!("Next is {}", qrs_analysis::pitch::note_label(d as f64)),
                th.text_2,
            ),
            (_, None) => ("The shape has no notes on this tuning".into(), th.miss),
        };
        txt(
            p,
            &msg,
            pos2(mid, y + px(34.0)),
            13.0,
            col,
            700,
            Align2::CENTER_TOP,
            Some(w),
        );
        let pr = &c.progress;
        txt(
            p,
            &format!(
                "{} run{} up and down  ·  {} clean",
                pr.runs,
                if pr.runs == 1 { "" } else { "s" },
                pr.clean
            ),
            pos2(mid, y + px(58.0)),
            12.0,
            if pr.runs > 0 { th.hit } else { th.caption },
            700,
            Align2::CENTER_TOP,
            Some(w),
        );
    }

    /// Over the song: how many of your notes were in the scale.
    pub(super) fn draw_jam(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) {
        let th = t();
        let mid = x + w / 2.0;
        let (big, col) = if c.jam_all == 0 {
            ("–".to_string(), th.caption)
        } else {
            let k = c.jam_in as f64 / c.jam_all as f64;
            (format!("{}%", pct(k)), if k >= 0.8 { th.hit } else { th.text })
        };
        txt(
            p,
            &big,
            pos2(mid, y + px(18.0)),
            30.0,
            col,
            800,
            Align2::CENTER_CENTER,
            None,
        );
        txt(
            p,
            &if c.jam_all == 0 {
                if self.has_input() {
                    "of your notes in the scale: play!".to_string()
                } else {
                    "no input: Settings (F2) > Audio input / output".to_string()
                }
            } else {
                format!("of your {} notes in the scale", c.jam_all)
            },
            pos2(mid, y + px(42.0)),
            12.0,
            th.text_2,
            700,
            Align2::CENTER_TOP,
            Some(w),
        );
        if let Some(&(m, _)) = c.jam.last() {
            let inn = in_scale(c.pcs, m);
            txt(
                p,
                &format!(
                    "{}  ·  {}",
                    qrs_analysis::pitch::note_label(m as f64),
                    if inn { "in the scale" } else { "outside it" }
                ),
                pos2(mid, y + px(62.0)),
                13.0,
                if inn { th.hit } else { th.miss },
                800,
                Align2::CENTER_TOP,
                Some(w),
            );
        }
    }

    /// The phrase as a row of keys, in its groups: the notes found show their fret (and string, when the riff
    /// crosses strings), the one being looked for is lit, the rest wait. Returns the y under it.
    fn draw_phrase(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) -> f32 {
        let th = t();
        let multi = c.strings().len() > 1;
        let (mut kw, kh) = (px(26.0), if multi { px(40.0) } else { px(32.0) });
        let (mut gap, mut group_gap) = (px(4.0), px(14.0));
        // narrower keys (down to 18 points) before a second row
        let groups = c
            .phrase
            .windows(2)
            .filter(|p| p[1].t - p[0].t > GROUP_GAP)
            .count();
        let n = c.phrase.len();
        let need = n as f32 * kw + (n.saturating_sub(1) - groups) as f32 * gap + groups as f32 * group_gap;
        if need > w && need > 0.0 {
            let k = (w / need).max(px(18.0) / kw);
            kw *= k;
            gap *= k;
            group_gap *= k;
        }
        let found = c.progress.found as usize;
        let looking = c.looking_for();
        let mut cx = x;
        let mut cy = y;
        for (i, n) in c.phrase.iter().enumerate() {
            if i > 0 {
                let new_group = n.t - c.phrase[i - 1].t > GROUP_GAP;
                cx += if new_group { group_gap } else { gap };
            }
            if cx + kw > x + w + 0.5 {
                // (half a point of slack: a row that fits exactly mustn't wrap on rounding)
                cx = x;
                cy += kh + px(8.0);
            }
            let r = Rect::from_min_size(pos2(cx, cy), vec2(kw, kh));
            if Some(i) == looking {
                paint::raised(p, r, Some(8.0), 2.0, Fill::Base, 0.0);
                paint::ring(p, r, Some(8.0), 1.5, th.accent, false);
                txt(
                    p,
                    "?",
                    r.center(),
                    15.0,
                    th.accent_ink,
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
            } else if i < found {
                paint::inset(p, r, Some(8.0), 2.0, Fill::Base);
                let fy = if multi { r.min.y + px(14.0) } else { r.center().y };
                txt(
                    p,
                    &n.fret.to_string(),
                    pos2(r.center().x, fy),
                    14.0,
                    th.text,
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
                if multi {
                    txt(
                        p,
                        &self.string_name(n.string),
                        pos2(r.center().x, r.max.y - px(10.0)),
                        10.0,
                        th.caption,
                        700,
                        Align2::CENTER_CENTER,
                        None,
                    );
                }
            } else {
                paint::inset(p, r, Some(8.0), 2.0, Fill::Base);
                paint::dot(p, r.center(), 2.0, th.unlit);
            }
            cx += kw;
        }
        let under = cy + kh + px(8.0);
        txt(
            p,
            &format!(
                "{} of {} found{}",
                found.min(c.phrase.len()),
                c.phrase.len(),
                if multi {
                    String::new()
                } else {
                    let s = self.coach_strings();
                    if c.riff.string_hint && !s.is_empty() {
                        format!("  ·  all on {s}")
                    } else {
                        String::new()
                    }
                }
            ),
            pos2(x, under),
            12.0,
            th.caption,
            700,
            Align2::LEFT_TOP,
            Some(w),
        );
        under + px(18.0)
    }

    /// What the last pluck was against the note being looked for (for a moment after it), what you're playing,
    /// and the hints given so far.
    fn draw_verdict(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32, hints: &[String]) {
        let th = t();
        let mid = x + w / 2.0;
        let fresh = c.said.filter(|s| s.2.elapsed().as_secs_f32() < SAID_FOR);
        let unclear = c.unclear.is_some_and(|u| u.elapsed().as_secs_f32() < SAID_FOR);
        let through = c.through.is_some_and(|u| u.elapsed().as_secs_f32() < SAID_FOR) && c.run > 0;
        let big = y + px(22.0);
        let (icon, head, sub, col): (Option<Icon>, String, String, Color32) = match fresh {
            _ if c.all_found() => (
                Some(Icon::Check),
                "Every note found".into(),
                "by ear".into(),
                th.hit,
            ),
            None if through => (
                None,
                "Playing it through".into(),
                match c.looking_for() {
                    Some(k) if c.run >= k => "now the new note".to_string(),
                    Some(k) => format!("{} of {k} so far, then the new one", c.run),
                    None => String::new(),
                },
                th.text_2,
            ),
            Some((_, Verdict::Match, _)) => (
                Some(Icon::Check),
                "Got it!".into(),
                "next one coming".into(),
                th.hit,
            ),
            Some((_, Verdict::Off(d), _)) => (
                Some(if d > 0 { Icon::Up } else { Icon::Down }),
                if d > 0 { "Higher" } else { "Lower" }.into(),
                if d.abs() <= 2 {
                    "a little"
                } else if d.abs() <= 5 {
                    "a few frets"
                } else {
                    "a long way"
                }
                .into(),
                th.text,
            ),
            Some((_, Verdict::Octave(k), _)) => (
                Some(if k > 0 { Icon::Up } else { Icon::Down }),
                "Right note, wrong octave".into(),
                format!("go {} an octave", if k > 0 { "up" } else { "down" }),
                th.late,
            ),
            None if unclear => (
                None,
                "Didn't catch that one".into(),
                "pick it clearly and let it ring".into(),
                th.caption,
            ),
            None => (None, "Play a note".into(), String::new(), th.caption),
        };
        let size = if icon.is_some() { 22.0 } else { 17.0 };
        let hw = text::width(p.ctx(), &head, Style::new(size, 800, col));
        let iw = if icon.is_some() { px(30.0) } else { 0.0 };
        let hx = mid - (hw + iw) / 2.0;
        if let Some(ic) = icon {
            icons::icon(p, ic, pos2(hx + px(12.0), big), 24.0, col);
        }
        txt(
            p,
            &head,
            pos2(hx + iw, big),
            size,
            col,
            800,
            Align2::LEFT_CENTER,
            Some(w),
        );
        let mut yy = big + px(22.0);
        if !sub.is_empty() {
            txt(
                p,
                &sub,
                pos2(mid, yy),
                12.0,
                th.caption,
                700,
                Align2::CENTER_CENTER,
                Some(w),
            );
        }
        yy += px(20.0);
        let you = match (fresh, self.tuner_text.is_empty()) {
            (Some((m, _, _)), _) => format!("you played {}", qrs_analysis::pitch::note_label(m)),
            (None, false) => format!("you: {}", self.tuner_text),
            (None, true) if !self.has_input() => "no input: Settings (F2) > Audio input / output".into(),
            (None, true) => String::new(),
        };
        if !you.is_empty() {
            txt(
                p,
                &you,
                pos2(mid, yy),
                13.0,
                th.text_2,
                700,
                Align2::CENTER_CENTER,
                Some(w),
            );
        }
        yy += px(26.0);
        let ctx = p.ctx().clone();
        for h in hints.iter().take(c.hints) {
            paint::dot(p, pos2(x + px(5.0), yy + px(9.0)), 2.5, th.accent);
            for line in text::wrap(&ctx, h, Style::new(13.0, 700, th.accent_ink), w - px(18.0)) {
                txt(
                    p,
                    &line,
                    pos2(x + px(18.0), yy),
                    13.0,
                    th.accent_ink,
                    700,
                    Align2::LEFT_TOP,
                    None,
                );
                yy += px(19.0);
            }
            yy += px(4.0);
        }
    }

    /// A finished item's card: a heading, then (label, value) lines.
    fn draw_finished(&self, p: &egui::Painter, head: &str, lines: &[(&str, String)], x: f32, y: f32, w: f32) {
        let th = t();
        let mut yy = y;
        txt(p, head, pos2(x, yy), 20.0, th.hit, 800, Align2::LEFT_TOP, Some(w));
        yy += px(32.0);
        let ctx = p.ctx().clone();
        for (k, v) in lines {
            caps(p, &k.to_uppercase(), pos2(x, yy), 10.0, Align2::LEFT_TOP, None);
            yy += px(16.0);
            for line in text::wrap(&ctx, v, Style::new(13.0, 700, th.text), w) {
                txt(p, &line, pos2(x, yy), 13.0, th.text, 700, Align2::LEFT_TOP, None);
                yy += px(19.0);
            }
            yy += px(6.0);
        }
    }

    /// The best passes in words: "94% at 100%, 88% at 85%" (fastest first).
    fn best_passes(c: &Coach) -> String {
        let mut best = c.progress.best.clone();
        best.sort_by_key(|b| std::cmp::Reverse(b.0));
        let s: Vec<String> = best
            .iter()
            .map(|(s, a)| format!("{}% at {s}%", pct(*a)))
            .collect();
        if s.is_empty() { "–".into() } else { s.join(", ") }
    }

    /// The riff found by ear: how it went, and where it's kept.
    fn draw_check(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) {
        let pr = &c.progress;
        let mins = (pr.seconds / 60.0).round().max(1.0) as i64;
        let lines = [
            (
                "Found by ear",
                format!(
                    "{} of {} notes",
                    pr.found.min(c.phrase.len() as u32),
                    c.phrase.len()
                ),
            ),
            (
                "On the way",
                format!(
                    "{} wrong, {} octave out, {} hint{}",
                    pr.wrong,
                    pr.octave,
                    pr.hints,
                    if pr.hints == 1 { "" } else { "s" }
                ),
            ),
            ("Best passes", Self::best_passes(c)),
            ("Time", format!("{mins} min")),
            (
                "Saved",
                format!("as a lick: {} · {}", c.lesson.label(), c.riff.name),
            ),
        ];
        self.draw_finished(p, "Riff learned!", &lines, x, y, w);
    }

    /// A learned item or a scale done: how it went.
    fn draw_done(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32) {
        let pr = &c.progress;
        let mins = (pr.seconds / 60.0).round().max(1.0) as i64;
        let mut lines: Vec<(&str, String)> = Vec::new();
        if c.riff.kind == Kind::Scale {
            if let Some(sc) = &c.riff.scale {
                lines.push(("The scale", sc.name.clone()));
            }
            lines.push((
                "Up and down",
                format!(
                    "{} run{}, {} clean",
                    pr.runs,
                    if pr.runs == 1 { "" } else { "s" },
                    pr.clean
                ),
            ));
            if c.jam_all > 0 {
                lines.push((
                    "Over the song",
                    format!(
                        "{}% of your {} notes in the scale",
                        pct(c.jam_in as f64 / c.jam_all as f64),
                        c.jam_all
                    ),
                ));
            }
            lines.push(("Time", format!("{mins} min")));
            self.draw_finished(p, "Scale learned!", &lines, x, y, w);
        } else {
            if let super::practise::Take::Done(o) = &c.take
                && let Some(a) = o.accuracy
            {
                lines.push(("The take", format!("{}% at {}%", pct(a), pct(o.at_speed))));
            }
            if pr.takes > 0 {
                lines.push(("Takes", pr.takes.to_string()));
            }
            lines.push(("Best passes", Self::best_passes(c)));
            lines.push(("Time", format!("{mins} min")));
            lines.push((
                "Saved",
                format!("as a lick: {} · {}", c.lesson.label(), c.riff.name),
            ));
            self.draw_finished(p, "Learned!", &lines, x, y, w);
        }
    }

    /// The item's teaching points (`how`) as a list from `y`, as many lines as fit above `bottom` (the last one
    /// shown ends in "…" when some don't fit).
    fn draw_how(&self, p: &egui::Painter, c: &Coach, x: f32, y: f32, w: f32, bottom: f32) {
        let lh = px(17.0);
        if c.riff.how.is_empty() || y + px(18.0) + lh > bottom {
            return;
        }
        let th = t();
        let ctx = p.ctx().clone();
        let st = Style::new(12.0, 600, th.text_2);
        // every line, with its y and whether it starts a point
        let mut lines: Vec<(String, f32, bool)> = Vec::new();
        let mut yy = y + px(18.0);
        for h in &c.riff.how {
            for (k, line) in text::wrap(&ctx, h, st, w - px(14.0)).into_iter().enumerate() {
                lines.push((line, yy, k == 0));
                yy += lh;
            }
            yy += px(3.0);
        }
        let fit = lines.iter().take_while(|l| l.1 + lh <= bottom).count();
        if fit == 0 {
            return;
        }
        caps(p, "HOW", pos2(x, y), 10.0, Align2::LEFT_TOP, None);
        for (i, (line, ly, first)) in lines.iter().take(fit).enumerate() {
            if *first {
                paint::dot(p, pos2(x + px(4.0), ly + px(8.0)), 2.0, th.accent);
            }
            let cut = i + 1 == fit && fit < lines.len();
            let shown = if cut {
                format!("{}…", line.trim_end_matches(['.', ',', ';', ':', ' ']))
            } else {
                line.clone()
            };
            txt(
                p,
                &shown,
                pos2(x + px(14.0), *ly),
                12.0,
                th.text_2,
                600,
                Align2::LEFT_TOP,
                Some(w - px(14.0)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tg(t: f64, string: i32, fret: i32) -> Target {
        Target {
            t,
            string,
            fret,
            midi: [40, 45, 50, 55, 59, 64][string as usize] + fret,
            sus: 0.0,
        }
    }

    fn chart_of(notes: &[(f64, i32, i32)]) -> Chart {
        use qrs_formats::{Arrangement, Note};
        let mut arr = Arrangement::new("Lead");
        arr.song_length = 30.0;
        for &(t_, s, f) in notes {
            arr.notes.push(Note::new(t_, s, f));
        }
        let nota = crate::player::Nota {
            show: true,
            snap: false,
            rests: true,
            beams: true,
            triplets: true,
        };
        Chart::new(arr, 30.0, &nota)
    }

    #[test]
    fn the_phrase_is_the_riffs_first_notes() {
        use qrs_formats::{Arrangement, Note};
        let mut arr = Arrangement::new("Lead");
        arr.song_length = 30.0;
        for (t_, s, f) in [
            (9.0, 3, 5),
            (10.0, 3, 0),
            (10.5, 3, 3),
            (11.0, 3, 5),
            (11.5, 2, 7),
            (12.0, 3, 0),
            (20.0, 3, 3),
        ] {
            arr.notes.push(Note::new(t_, s, f));
        }
        arr.notes[4].techniques = Techs::FRET_HAND_MUTE; // a muted note has no pitch to find
        let nota = crate::player::Nota {
            show: true,
            snap: false,
            rests: true,
            beams: true,
            triplets: true,
        };
        let chart = Chart::new(arr, 30.0, &nota);
        let riff = Riff {
            start: 10.0,
            end: 15.0,
            phrase: 3,
            ..Default::default()
        };
        let ph = phrase_of(&chart, &riff);
        assert_eq!(
            ph.iter().map(|n| (n.t, n.fret, n.midi)).collect::<Vec<_>>(),
            vec![(10.0, 0, 55), (10.5, 3, 58), (11.0, 5, 60)]
        );
        // every note in the stretch: the muted one left out, the one after the end too
        let all = phrase_of(
            &chart,
            &Riff {
                phrase: 0,
                ..riff.clone()
            },
        );
        assert_eq!(
            all.iter().map(|n| n.t).collect::<Vec<_>>(),
            vec![10.0, 10.5, 11.0, 12.0]
        );
        // the stretch's places on the neck, each once (the muted note's too: it isn't drawn)
        assert_eq!(stretch_spots(&chart, &riff), vec![(3, 0), (3, 3), (3, 5)]);
    }

    #[test]
    fn a_long_stretch_has_every_note_and_a_capped_phrase() {
        // 48 s of eighth notes on the A string
        let notes: Vec<(f64, i32, i32)> = (0..192).map(|i| (2.0 + i as f64 * 0.25, 1, i % 5)).collect();
        let chart = chart_of(&notes);
        let riff = Riff {
            start: 1.95,
            end: 50.1,
            phrase: 16,
            ..Default::default()
        };
        assert_eq!(phrase_of(&chart, &riff).len(), 16);
        assert_eq!(
            phrase_of(
                &chart,
                &Riff {
                    phrase: 0,
                    ..riff.clone()
                }
            )
            .len(),
            192
        );
        assert_eq!(stretch_spots(&chart, &riff).len(), 5);
    }

    #[test]
    fn playing_the_riff_through_to_the_new_note_isnt_wrong() {
        // Seven Nation Army: E E G E D C B, looking for the D (note 5) after four found
        let ph = [(1, 7), (1, 7), (1, 10), (1, 7), (1, 5), (1, 3), (1, 2)]
            .iter()
            .enumerate()
            .map(|(i, &(s, f))| tg(i as f64 * 0.4, s, f))
            .collect::<Vec<_>>();
        let (e3, g3, d3, a3) = (52.0, 55.0, 50.0, 57.0);
        assert_eq!(hear_pluck(&ph, 4, 0, d3), Heard::Judged(Verdict::Match));
        assert_eq!(hear_pluck(&ph, 4, 0, a3), Heard::Judged(Verdict::Off(-7)));
        // from the start: E (starts it), E, G, E, then the D is found
        assert_eq!(hear_pluck(&ph, 4, 0, e3), Heard::Through(1));
        assert_eq!(hear_pluck(&ph, 4, 1, e3), Heard::Through(2));
        assert_eq!(hear_pluck(&ph, 4, 2, g3), Heard::Through(3));
        assert_eq!(hear_pluck(&ph, 4, 3, e3), Heard::Through(4));
        assert_eq!(hear_pluck(&ph, 4, 4, d3), Heard::Judged(Verdict::Match));
        // a wrong turn in a run-through is judged against the note looked for
        assert_eq!(hear_pluck(&ph, 4, 2, a3), Heard::Judged(Verdict::Off(-7)));
        // the first note can't start a run-through while it's the one looked for
        assert_eq!(hear_pluck(&ph, 0, 0, a3), Heard::Judged(Verdict::Off(-5)));
        // Smoke on the Water's fourth note is its first again: found, not a run-through
        let smoke = [(3, 0), (3, 3), (3, 5), (3, 0)]
            .iter()
            .enumerate()
            .map(|(i, &(s, f))| tg(i as f64 * 0.5, s, f))
            .collect::<Vec<_>>();
        assert_eq!(hear_pluck(&smoke, 3, 0, 55.0), Heard::Judged(Verdict::Match));
    }

    #[test]
    fn steps_follow_the_stored_keys() {
        for kind in [Kind::Ear, Kind::Learn, Kind::Scale] {
            let riff = Riff {
                kind,
                ..Default::default()
            };
            let steps = Step::list(&riff);
            // the coach's steps are the library's, in its order
            assert_eq!(
                steps.iter().map(|s| s.key()).collect::<Vec<_>>(),
                kind.steps().iter().map(|s| s.key).collect::<Vec<_>>()
            );
            for (s, info) in steps.iter().zip(kind.steps()) {
                assert_eq!(Step::from_key(&riff, &s.key()), *s);
                assert_eq!(s.title(), info.title);
            }
            // v1's tune step, none, or another kind's: the first step
            assert_eq!(Step::from_key(&riff, "tune"), steps[0]);
            assert_eq!(Step::from_key(&riff, ""), steps[0]);
            assert_eq!(Step::from_key(&riff, "nonsense"), steps[0]);
            assert!(steps.last().unwrap().last());
        }
        let of = |kind: Kind, k: &str| {
            Step::from_key(
                &Riff {
                    kind,
                    ..Default::default()
                },
                k,
            )
        };
        assert_eq!(of(Kind::Ear, "tune"), Step::Listen);
        assert_eq!(of(Kind::Learn, "notes"), Step::Listen);
        assert_eq!(of(Kind::Scale, "run"), Step::Run);
        // a learn item left at v2's Learn it or Play along carries on at Practise
        assert_eq!(of(Kind::Learn, "learn"), Step::Practise);
        assert_eq!(of(Kind::Learn, "play"), Step::Practise);
        assert_eq!(of(Kind::Learn, "record"), Step::Record);
        // a theory item's pages are its steps
        let page = |t: &str| qrs_library::lessons::Page {
            title: t.into(),
            ..Default::default()
        };
        let th = Riff {
            kind: Kind::Theory,
            pages: vec![page("The key"), page("The chords"), page("The scale")],
            ..Default::default()
        };
        assert_eq!(Step::list(&th), vec![Step::Page(0), Step::Page(1), Step::Page(2)]);
        assert_eq!(Step::from_key(&th, "p1"), Step::Page(1));
        assert_eq!(Step::from_key(&th, "p7"), Step::Page(2));
        assert_eq!(Step::from_key(&th, "listen"), Step::Page(0));
        assert_eq!(Step::Page(2).key(), "p2");
        let c = Coach::new(Arc::new(Lesson::default()), 0, th, RiffProgress::default());
        assert_eq!(c.step_title(Step::Page(1)), "The chords");
        assert_eq!(c.step_title(Step::Listen), "Listen");
    }

    #[test]
    fn play_along_starts_at_full_speed_and_finishes_there() {
        let riff = Riff {
            speeds: vec![0.7, 1.0, 0.55, 0.85],
            ..Default::default()
        };
        let ladder = riff.ladder();
        assert_eq!(ladder, vec![1.0, 0.85, 0.7, 0.55]);
        let c = Coach::new(Arc::new(Lesson::default()), 0, riff, RiffProgress::default());
        assert_eq!(c.speed_i, 0);
        assert_eq!(c.speed(), 1.0);
        // Slower and Faster within the ladder
        assert_eq!(notch(0, 4, 1), 1);
        assert_eq!(notch(3, 4, 1), 3, "no slower than the slowest");
        assert_eq!(notch(2, 4, -1), 1);
        assert_eq!(notch(0, 4, -1), 0, "no faster than full speed");
        assert_eq!(notch(0, 1, 1), 0);
        // a pass below the mark stays; a good one below full speed moves up a notch; at full speed it's done
        assert_eq!(after_pass(2, 0.85, 0.9), Some(2));
        assert_eq!(after_pass(2, 0.9, 0.9), Some(1));
        assert_eq!(after_pass(1, 0.95, 0.9), Some(0));
        assert_eq!(after_pass(0, 0.89, 0.9), Some(0));
        assert_eq!(after_pass(0, 0.9, 0.9), None);
        assert_eq!(after_pass(0, 0.8, 0.8), None, "the item's own mark");
    }

    #[test]
    fn a_scale_run_up_then_down() {
        // E minor pentatonic in the open position: E G A B D E G A B D E G (up), then down without the top twice
        let sc = qrs_library::lessons::Scale {
            name: "E minor pentatonic".into(),
            root: "E".into(),
            shape: vec![
                (0, 0),
                (0, 3),
                (1, 0),
                (1, 2),
                (2, 0),
                (2, 2),
                (3, 0),
                (3, 2),
                (4, 0),
                (4, 3),
                (5, 0),
                (5, 3),
            ],
        };
        let seq = sc.run(&[40, 45, 50, 55, 59, 64]);
        assert_eq!(seq.len(), 23);
        // up: each note moves it on
        let mut pos = 0;
        for &m in &seq[..12] {
            assert_eq!(run_pluck(&seq, pos, m as f64 + 0.2), RunHeard::Next);
            pos += 1;
        }
        // the top note again (a second pluck) changes nothing; then down
        assert_eq!(run_pluck(&seq, pos, 67.0), RunHeard::Again);
        assert_eq!(seq[pos], 64, "the first note down is the one under the top");
        // a wrong note: the note due stays due
        assert_eq!(run_pluck(&seq, pos, 65.0), RunHeard::Wrong);
        // the same pitch fretted elsewhere (B string 5th fret = E4) counts
        assert_eq!(run_pluck(&seq, pos, 64.0), RunHeard::Next);
        pos += 1;
        for &m in &seq[pos..22] {
            assert_eq!(run_pluck(&seq, pos, m as f64), RunHeard::Next);
            pos += 1;
        }
        // the low E ends it
        assert_eq!(run_pluck(&seq, pos, 40.0), RunHeard::Done);
        // part way up, the first note again starts over
        assert_eq!(run_pluck(&seq, 5, 40.0), RunHeard::Over);
        assert_eq!(run_pluck(&seq, 0, 41.0), RunHeard::Wrong);
        assert_eq!(run_pluck(&[], 0, 41.0), RunHeard::Wrong);
    }

    #[test]
    fn the_box_shows_the_shape_and_the_notes_near_it() {
        let b7 = [(0, 7), (0, 10), (5, 7), (5, 10)];
        assert_eq!(box_frets(&b7, &[]), (6, 11));
        // the riff's notes a fret or two outside widen it; far ones don't
        assert_eq!(box_frets(&b7, &[(3, 12), (1, 2)]), (6, 13));
        // near the nut: from the nut
        assert_eq!(box_frets(&[(0, 0), (0, 3)], &[]), (0, 5));
        assert_eq!(box_frets(&[(0, 2), (0, 5)], &[]), (0, 6));
        assert_eq!(box_frets(&[], &[]), (0, 5));
    }

    #[test]
    fn in_scale_or_out() {
        let sc = qrs_library::lessons::Scale {
            name: "A minor pentatonic".into(),
            root: "A".into(),
            shape: vec![(0, 5), (0, 8), (1, 5), (1, 7), (2, 5), (2, 7)],
        };
        let pcs = sc.pitch_classes(&[40, 45, 50, 55, 59, 64]);
        // A C D E G anywhere on the neck; Bb and F# out
        for m in [45, 48, 50, 52, 55, 57, 72, 79] {
            assert!(in_scale(pcs, m), "{m}");
        }
        for m in [46, 54, 41] {
            assert!(!in_scale(pcs, m), "{m}");
        }
    }

    #[test]
    fn hear_it_stops_right_after_the_note() {
        // Smoke on the Water's first bar: G, Bb, C (held)
        let ph = vec![
            tg(12.136, 3, 0),
            tg(12.673, 3, 3),
            Target {
                sus: 0.54,
                ..tg(13.229, 3, 5)
            },
            tg(14.042, 3, 0),
        ];
        // the first note alone, up to just before the second
        let (a, b) = hear_span(&ph, 0).unwrap();
        assert!(
            (a - 12.056).abs() < 1e-9 && b < 12.673 && b > 12.136 + 0.3,
            "{a} {b}"
        );
        // the third: from the first, through its ring, stopping before the fourth
        let (a, b) = hear_span(&ph, 2).unwrap();
        assert!((a - (12.136 - 0.15)).abs() < 1e-9, "{a}");
        assert!((b - (13.229 + 0.54)).abs() < 1e-9, "{b}");
        // the last: rings a little longer
        let (_, b) = hear_span(&ph, 3).unwrap();
        assert!((b - (14.042 + 0.5)).abs() < 1e-9, "{b}");
        assert!(hear_span(&ph, 4).is_none());
        // a note long after the riff's start: only the last few seconds before it
        let far = vec![tg(10.0, 1, 7), tg(13.0, 1, 7), tg(16.0, 1, 10)];
        let (a, _) = hear_span(&far, 2).unwrap();
        assert!((a - (13.0 - 0.15)).abs() < 1e-9, "{a}");
    }

    #[test]
    fn hints_go_from_gentle_to_the_fret() {
        let first = tg(0.0, 3, 5);
        let h = hints_for(&first, None, true, "G string");
        assert_eq!(
            h,
            vec![
                "It's between fret 3 and fret 7 on the G string.".to_string(),
                "It's fret 5 on the G string.".to_string()
            ]
        );
        let h = hints_for(&first, None, false, "G string");
        assert_eq!(h[0], "It's on the G string.");
        let prev = tg(0.0, 1, 7);
        let same = hints_for(&tg(1.0, 1, 7), Some(&prev), true, "A string");
        assert_eq!(same[0], "It's the same note as the one before.");
        let up = hints_for(&tg(1.0, 1, 10), Some(&prev), true, "A string");
        assert_eq!(up[0], "It's higher than the note before: a jump.");
        assert_eq!(up[1], "It's 3 frets higher than the note before.");
        assert_eq!(up[2], "It's fret 10 on the A string.");
        let across = hints_for(&tg(1.0, 2, 5), Some(&prev), false, "D string");
        assert_eq!(across[2], "It's 3 semitones higher than the note before.");
        let open = hints_for(&tg(1.0, 3, 0), Some(&tg(0.0, 3, 3)), true, "G string");
        assert_eq!(open.last().unwrap(), "It's the open G string.");
    }
}
