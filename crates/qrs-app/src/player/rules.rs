//! The practice rules (new in quickRS, P37.11, owner request; D119-D121): how the practice features go together,
//! kept in one place so each feature asks here instead of checking the others itself.
//! - The practice range (D119): one stretch being practised, `loop_a`..`loop_b`. A Ctrl-drag on the tab, `[` `]`,
//!   L (a section), a review spot or a saved lick each set it (`set_range`), replacing the one before; `\`, Ctrl+[,
//!   the selection's x or Esc clear it (`clear_range`), with continuous mode and the review spot that need it.
//!   Setting a range with the playhead outside it takes the playhead to its lead-in. The loop catches the song only
//!   when it plays out of the range's end (a seek past it plays on from there; the range waits); Home in a range is
//!   its lead-in, Home again the song's start; a jump to another section moves a section's loop with it.
//! - Speed (D120): whatever plays the song plays at the speed set: the song and its loops, a review spot, the
//!   selection's player (restarted at the new speed), a take listened back (made again at it). The speed keys work
//!   with the review page up. A take being recorded stops on a speed change (it's scored at one speed).
//! - The count-in (D121): config `countdown` (on by default). Every "play from here" that counted in goes through
//!   `count_in`: a song opening, the first Play, a review spot, continuous, the whole song again, a take, a
//!   lesson's step, closing the review onto a song that was playing. A seek during a count-in keeps counting in
//!   from the new place. Off, they all play at once.
//! - The review page pauses the song and remembers where it was; closed (E, Esc, its x) it's back there, the
//!   range as it was, playing again (after the count-in) if it was. A spot's loop replaces your range while you
//!   practise it; leaving the spot (its x) gives your range back. Not over a take being recorded (it says so).
//! - Modes that can't run together say so: a take with note by note on (note by note waits until the take is
//!   over), the review during a take; a seek stops a take being listened back.

use std::time::{Duration, Instant};

use super::{COUNTDOWN, LOOP_LEAD_IN, PlayerView};
use crate::state::AppState;

/// Where the practice range came from (what follows it: a section's loop moves with a jump to another section).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RangeFrom {
    /// `[` and `]`.
    Marks,
    /// L: this section (its index).
    Section(usize),
    /// A Ctrl-drag across the tab (the tab's selection shows it).
    Selection,
    /// A review spot.
    Spot,
    /// A saved lick.
    Lick,
}

/// What the player was doing when the review page opened: back to it when the page closes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ReviewReturn {
    /// Song seconds.
    pub pos: f64,
    pub playing: bool,
}

/// Your range before a review spot replaced it (given back when you leave the spots).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RangeBefore {
    pub range: Option<(f64, f64, RangeFrom)>,
    /// Song seconds.
    pub pos: f64,
}

/// Home within this of the range's lead-in goes to the song's start.
const HOME_AGAIN: f64 = 0.3;

impl PlayerView {
    // -- the count-in (D121) ---------------------------------------------------------------------------------

    /// Play from where the song is: after the count-in when it's on, else at once.
    pub(super) fn count_in(&mut self) {
        self.fresh = false;
        if self.countdown_on {
            self.countdown_until = Some(Instant::now() + Duration::from_secs_f32(COUNTDOWN));
        } else {
            self.countdown_until = None;
            if !self.transport.playing() {
                self.transport.play();
            }
        }
    }

    /// The count-in key (Shift C): on or off everywhere, kept in config.json.
    pub(super) fn toggle_countdown(&mut self, st: &mut AppState) {
        self.countdown_on = !self.countdown_on;
        st.config.set("countdown", self.countdown_on);
        if !self.countdown_on && self.countdown_until.take().is_some() {
            // (counting in now: play at once)
            self.transport.play();
        }
        if self.countdown_on {
            self.say(
                "Count-in on: ",
                "3, 2, 1 before the song, a loop, a review spot or a take",
            );
        } else {
            self.say("Count-in off: ", "everything plays at once");
        }
    }

    // -- the practice range (D119) ---------------------------------------------------------------------------

    /// The range being practised, if there's one.
    pub(super) fn range(&self) -> Option<(f64, f64)> {
        self.loop_a.zip(self.loop_b).filter(|(a, b)| b > a)
    }

    /// Practise [a, b] (chart seconds): it replaces the range (and a selection that isn't it); its passes start
    /// again; with the playhead outside it, the playhead goes to its lead-in (playing on, or stopped there).
    pub(super) fn set_range(&mut self, a: f64, b: f64, from: RangeFrom) {
        if from != RangeFrom::Selection {
            self.tab.sel = None;
        }
        if from != RangeFrom::Spot
            && let Some(r) = self.review.as_mut()
        {
            r.current = None; // (another range: no longer the spot's)
        }
        if from != RangeFrom::Spot {
            self.range_before = None;
        }
        self.stop_synth();
        self.loop_a = Some(a);
        self.loop_b = Some(b);
        self.range_from = Some(from);
        self.loop_passes = 0;
        self.loop_acc = None;
        let now = self.now();
        if now < a - LOOP_LEAD_IN - 0.05 || now >= b {
            self.seek((a - LOOP_LEAD_IN).max(0.0) + self.offset);
        }
    }

    /// No range: the loop, the selection, continuous mode and the review's spot all go.
    pub(super) fn clear_range(&mut self) {
        self.loop_a = None;
        self.loop_b = None;
        self.range_from = None;
        self.range_before = None;
        self.loop_passes = 0;
        self.loop_acc = None;
        self.continuous = false;
        if self.tab.sel.take().is_some() {
            self.stop_synth();
        }
        if let Some(r) = self.review.as_mut() {
            r.current = None;
        }
    }

    /// The range as it is now (with where it came from), to give back later.
    pub(super) fn range_now(&self) -> Option<(f64, f64, RangeFrom)> {
        let (a, b) = self.range()?;
        Some((a, b, self.range_from.unwrap_or(RangeFrom::Marks)))
    }

    /// Every frame while playing: the song played out of the range's end goes round. One that jumped past the end
    /// (a seek: `prev` is none or already past it) plays on.
    pub(super) fn loop_due(&self, prev: Option<f64>, now: f64) -> bool {
        let Some((a, b)) = self.range() else {
            return false;
        };
        now >= b && prev.is_some_and(|p| p < b && p >= a - LOOP_LEAD_IN - 1.0)
    }

    /// Home: the range's lead-in (again: the song's start), else the song's start.
    pub(super) fn restart(&mut self) {
        if let Some((a, _)) = self.range() {
            let lead = (a - LOOP_LEAD_IN).max(0.0);
            if (self.now() - lead).abs() > HOME_AGAIN {
                self.seek(lead + self.offset);
                self.say("Restart the loop", "  \u{b7}  Home again: the song's start");
                return;
            }
        }
        self.seek(0.0);
        self.say("Restart", "");
    }

    /// A jump to section `i` (the menu, Page Up / Down): a section's loop moves with it.
    pub(super) fn range_follows_section(&mut self, i: usize) {
        if !matches!(self.range_from, Some(RangeFrom::Section(_))) || self.range().is_none() {
            return;
        }
        let Some(sec) = self.chart.sections.get(i).cloned() else {
            return;
        };
        self.loop_a = Some(sec.start);
        self.loop_b = Some(sec.end);
        self.range_from = Some(RangeFrom::Section(i));
        self.loop_passes = 0;
        self.loop_acc = None;
    }

    // -- speed (D120) ------------------------------------------------------------------------------------------

    /// A new speed took over: what else is playing the song follows it.
    pub(super) fn speed_applied(&mut self) {
        if self.synth.state != qrs_audio::synth::SynthState::Idle && self.tab.sel.is_some() {
            self.play_selection(false); // (the selection again, at the new speed)
        }
        if self.take.as_ref().is_some_and(super::take::Session::listening) {
            self.take_listen_again();
        }
    }

    // -- the review page ---------------------------------------------------------------------------------------

    /// Whether the review page may open now (not over a take being recorded: it says so).
    pub(super) fn review_may_open(&mut self) -> bool {
        if self.take.as_ref().is_some_and(super::take::Session::going) {
            self.say("The review waits for the take: ", "finish it, or Ctrl R stops it");
            return false;
        }
        true
    }

    /// The review page opens: where the song is and whether it's playing, kept for when it closes.
    pub(super) fn review_opening(&mut self) {
        self.stop_synth(); // (nothing plays under the page)
        if self.review_return.is_none() {
            self.review_return = Some(ReviewReturn {
                pos: self.transport.position(),
                playing: self.transport.playing() || self.countdown_until.is_some(),
            });
        }
    }

    /// The review page closed without a spot picked: back where the song was, playing again if it was.
    pub(super) fn review_closed(&mut self) {
        let Some(r) = self.review_return.take() else {
            return;
        };
        if (self.transport.position() - r.pos).abs() > 0.01 {
            self.seek(r.pos);
        }
        if r.playing {
            self.count_in();
        }
    }

    /// A spot is picked: your range kept (at the first spot) to give back when you leave the spots.
    pub(super) fn spot_entering(&mut self) {
        let at_spot =
            self.review.as_ref().is_some_and(|r| r.current.is_some()) && self.range_before.is_some();
        if !at_spot {
            self.range_before = Some(RangeBefore {
                range: self.range_now(),
                pos: self.review_return.map_or(self.transport.position(), |r| r.pos),
            });
        }
        self.review_return = None; // (the spot plays now, not what was playing)
    }

    /// Leaving the spots: your range back (or none), the playhead where it was before them.
    pub(super) fn spot_left(&mut self) {
        let before = self.range_before.take();
        self.continuous = false;
        if let Some(r) = self.review.as_mut() {
            r.current = None;
        }
        match before.and_then(|b| b.range.map(|r| (r, b.pos))) {
            Some(((a, b, from), _)) => {
                if from == RangeFrom::Selection {
                    self.tab.sel = Some((a, b));
                }
                self.set_range(a, b, from);
            }
            None => {
                self.clear_range();
                if let Some(b) = before {
                    self.seek(b.pos);
                }
            }
        }
    }
}
