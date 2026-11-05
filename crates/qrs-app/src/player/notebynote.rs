//! Note by note (new in quickRS, P15.3, owner request; no Python source): the song stops at each note still to be
//! played and waits until you play it (qrs-analysis `Scorer::wait_on`: the right note or chord, whenever you pluck
//! it), then goes on. Played in time it never stops. W or the jewel lamp above the transport turns it on; Enter
//! skips the note it's waiting at; Space pauses as usual. In a lesson the coach has it (P16.3, P17.2): a learn
//! item's Practise step turns it on and off (W), waiting only at the item's own notes; off in every other step. Not
//! a scored run (a Practise pass with it on isn't counted).

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, CursorIcon, Rect, Sense, Ui, pos2};
use qrs_analysis::detect::{EARLY, Live, NoteGroup};
use qrs_analysis::{Held, Pluck};
use qrs_ui::keys::jewel_lamp;
use qrs_ui::text;
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::interact;

use super::PlayerView;

/// The song stops this far (chart seconds) before a note it waits at, so on the way again the record's note is heard
/// whole, with yours.
const HOLD_LEAD: f64 = 0.03;
/// A pluck still being read when the song reaches a note may be that note, played in time: the song goes on up to
/// this far past it for the verdict before stopping (and going back to it).
const HOLD_GRACE: f64 = 0.15;
/// How long a wrong note shows (seconds).
const WRONG_FOR: f32 = 0.7;
/// Seconds for one breath of the lamp while it waits.
const BREATH: f32 = 1.6;
/// The stone's diameter (points).
const LAMP_D: f32 = 12.0;
/// The lamp's name, beside it.
const NAME: &str = "note by note"; // (as Controls names it: P37.6b)

/// The note the song is waiting at.
#[derive(Clone, Copy, Debug)]
pub struct Waiting {
    /// Its group in the scorer.
    pub gi: usize,
    /// The capture index when the song stopped (only plucks from then on count as wrong).
    since: u64,
    at: Instant,
    wrong: Option<Instant>,
}

impl Waiting {
    /// 0..1, breathing while it waits.
    fn swell(&self) -> f32 {
        let ph = self.at.elapsed().as_secs_f32() / BREATH;
        0.5 - 0.5 * (ph * std::f32::consts::TAU).cos()
    }

    /// 1 just after a wrong note, fading to 0.
    pub(super) fn wrong(&self) -> f32 {
        self.wrong
            .map(|w| 1.0 - w.elapsed().as_secs_f32() / WRONG_FOR)
            .unwrap_or(0.0)
            .max(0.0)
    }
}

/// The note group the song stops at, at chart time `now`: the first still to be played from just behind the line to
/// `HOLD_LEAD` ahead of it, within `span` (chart seconds: a lesson's stretch, else the whole song); none yet while a
/// pluck being read (`in_flight(from)`: one heard at `from` or later) may be it, until `HOLD_GRACE` past it.
pub fn hold_at(
    groups: &[NoteGroup],
    now: f64,
    span: (f64, f64),
    in_flight: impl Fn(f64) -> bool,
) -> Option<usize> {
    let i0 = groups.partition_point(|g| g.t < (now - HOLD_GRACE - 0.05).max(span.0 - 0.02));
    let gi = (i0..groups.len())
        .take_while(|&i| groups[i].t <= now + HOLD_LEAD && groups[i].t < span.1 - 0.01)
        .find(|&i| groups[i].state.is_none())?;
    let t = groups[gi].t;
    if now < t + HOLD_GRACE && in_flight(t - EARLY) {
        return None;
    }
    Some(gi)
}

impl PlayerView {
    /// Turn note by note on or off (off lets go of a note it was waiting at, and the song carries on).
    pub fn set_note_by_note(&mut self, on: bool) {
        if self.coach.is_some() {
            // (in a lesson it's Practise's: practise.rs)
            let _ = on;
            self.practise_nbn();
            return;
        }
        self.nbn = on;
        if on {
            if self.started_playing() {
                self.run_scored = false;
            }
            if self.has_input() {
                self.say(
                    "Note by note: ",
                    "the song waits for each note  ·  W turns it off",
                );
            } else {
                self.say("Note by note listens to your instrument: ", "no input right now");
            }
        } else {
            self.nbn_release(true);
            self.say("Note by note off", "");
        }
    }

    /// Let go of the note the song was waiting at; `play` carries the song on (otherwise it stays stopped).
    pub(super) fn nbn_release(&mut self, play: bool) {
        if self.waiting.take().is_some() && play {
            self.transport.play();
        }
    }

    /// Every frame: when the song reaches a note still to be played, stop it there. In a lesson the mode is on only
    /// in Practise with its note by note on (the item's own notes only); the guitar lost while the song waits lets
    /// it play on (as the song keeps time without it).
    pub(super) fn nbn_watch(&mut self) {
        if self.take.as_ref().is_some_and(super::take::Session::going) {
            // (a take is played through: the mode stands aside until it's over)
            self.nbn_release(true);
            return;
        }
        let mut span = (f64::NEG_INFINITY, f64::INFINITY);
        if let Some(c) = self.coach.as_ref() {
            if c.step != super::coach::Step::Practise || !c.nbn || c.tuner {
                self.nbn = false;
                self.waiting = None;
                return;
            }
            self.nbn = true;
            span = (c.riff.start, c.riff.end);
        }
        if !self.has_input() {
            self.nbn_release(true);
            return;
        }
        if !self.nbn || self.waiting.is_some() || !self.transport.playing() {
            return;
        }
        let now = self.now();
        let Some(gi) = hold_at(&self.scorer.groups, now, span, |from| self.scorer.in_flight(from)) else {
            return;
        };
        let t = self.scorer.groups[gi].t;
        self.transport.pause();
        self.transport.seek(t - HOLD_LEAD + self.offset); // the note on the line
        let since = self.engine().map(|e| e.capture().written()).unwrap_or(0);
        self.waiting = Some(Waiting {
            gi,
            since,
            at: Instant::now(),
            wrong: None,
        });
    }

    /// While the song waits: each pluck checked against the note; played, the song goes on.
    pub(super) fn nbn_listen(&mut self, e: &qrs_audio::Engine, onsets: &[qrs_audio::rt::Onset]) {
        let Some(w) = self.waiting else { return };
        let Some(t) = self.scorer.groups.get(w.gi).map(|g| g.t) else {
            self.nbn_release(true);
            return;
        };
        self.scorer.feed(onsets.iter().map(|o| Pluck {
            index: o.index,
            song_time: t,
        }));
        let live = Live {
            source: e.capture(),
            level: e.meters().level,
            latency: 0.0,
        };
        let wall = qrs_audio::clock::now_ns() as f64 / 1e9;
        match self.scorer.wait_on(w.gi, w.since, &live, wall) {
            Held::Played => self.nbn_release(true),
            Held::Wrong => {
                if let Some(w) = self.waiting.as_mut() {
                    w.wrong = Some(Instant::now());
                }
            }
            Held::Waiting => {}
        }
    }

    /// Enter while the song waits: pass the note by (it counts as missed) and go on.
    pub(super) fn nbn_skip(&mut self) {
        if let Some(w) = self.waiting {
            self.scorer.pass(w.gi, qrs_audio::clock::now_ns() as f64 / 1e9);
            self.nbn_release(true);
        }
    }

    /// Screenshot mode: note by note on, the song stopped at the first note after the moment shown, part way
    /// through a breath (`wrong`: a wrong note just played).
    pub(super) fn nbn_shot(&mut self, wrong: bool) {
        self.nbn = true;
        let now = self.now();
        let Some(gi) = self
            .scorer
            .groups
            .iter()
            .position(|g| g.t >= now && g.state.is_none())
        else {
            return;
        };
        let t = self.scorer.groups[gi].t;
        self.transport.pause();
        self.transport.hold(Some(t - HOLD_LEAD + self.offset));
        let now_i = Instant::now();
        self.waiting = Some(Waiting {
            gi,
            since: 0,
            at: now_i - Duration::from_secs_f32(BREATH * 0.3),
            wrong: wrong.then(|| now_i + Duration::from_secs(60)),
        });
    }

    /// For the tab: the group waited at, its breath (0..1) and a wrong note's flash (0..1).
    pub(super) fn nbn_tab(&self) -> Option<(usize, f32, f32)> {
        self.waiting.map(|w| (w.gi, w.swell(), w.wrong()))
    }

    /// The jewel lamp, first on the lamps' row (its slot `x`, `w` wide, at `y`; P37.11): dark when off, a lit ruby
    /// when on, breathing while the song waits for you; what for in its name's place then (back to `lo` at most).
    pub(super) fn draw_nbn_lamp(&mut self, ui: &Ui, x: f32, w: f32, y: f32, compact: bool, lo: f32) {
        let th = t();
        let p = ui.painter();
        let c = pos2(x + w - px(8.0), y);
        // its name left of it (owner request): quiet lowercase, as the seek bar's "bar 9 of 141", and clickable
        let name = text::Style::new(12.0, 500, th.caption);
        let hit = Rect::from_min_max(
            pos2(x - px(2.0), y - px(super::footer_keys::ROW_HALF)),
            pos2(x + w + px(2.0), y + px(super::footer_keys::ROW_HALF)),
        );
        let resp = interact(ui, hit, "nbn-lamp", Sense::click());
        let lesson = self
            .coach
            .as_ref()
            .is_some_and(|c| c.step != super::coach::Step::Practise);
        let on = self.nbn;
        let target = match self.waiting {
            Some(w) if on => 1.0 + w.swell(),
            _ if on => 1.0,
            _ => 0.0,
        };
        // on and off ease over about a tenth of a second
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        self.nbn_lit = if (target - self.nbn_lit).abs() < 0.02 || target > 1.0 {
            target
        } else {
            self.nbn_lit + (target - self.nbn_lit) * (dt * 14.0).min(1.0)
        };
        let hot = resp.hovered() && !lesson;
        let lamp = jewel_lamp(p, c, LAMP_D, self.nbn_lit, hot);
        let at = pos2(lamp.min.x - px(7.0), c.y);
        let max_w = Some((at.x - lo).max(0.0));
        match self.waiting {
            // while the song waits, what for, in its place
            Some(w) if on => {
                let (word, col) = if w.wrong() > 0.0 {
                    ("NOT THAT ONE", th.miss)
                } else {
                    ("YOUR NOTE  \u{b7}  ENTER SKIPS", th.accent_ink)
                };
                text::caps(p, word, at, 9.0, col, Align2::RIGHT_CENTER, max_w);
            }
            // brightening as the lamp lights (or under the pointer)
            _ if compact => {}
            _ => {
                let lit = if hot { 1.0 } else { self.nbn_lit.min(1.0) };
                let col = qrs_ui::raster::mix(th.caption, th.text, lit);
                text::draw(
                    p,
                    NAME,
                    at,
                    Align2::RIGHT_CENTER,
                    text::Style { color: col, ..name },
                    max_w,
                );
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(if lesson {
                CursorIcon::NotAllowed
            } else {
                CursorIcon::PointingHand
            });
            let tip = if lesson {
                "Note by note: in a lesson, Practise has it (W)"
            } else if on {
                "Note by note: the song waits for each note"
            } else {
                "Note by note: the song waits for each note you play"
            };
            self.tip = Some((
                tip.into(),
                if lesson { String::new() } else { "W".into() },
                pos2(x, hit.min.y - px(4.0)),
                Align2::LEFT_BOTTOM,
            ));
        }
        if resp.clicked() && !lesson {
            self.acts.push("nbn".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrs_analysis::{Judgement, Scorer};
    use qrs_formats::model::{Arrangement, Note};

    fn groups(times: &[f64]) -> Vec<NoteGroup> {
        let mut a = Arrangement::new("Lead");
        a.notes = times.iter().map(|&t| Note::new(t, 0, 3)).collect();
        Scorer::new(&a).groups
    }

    #[test]
    fn the_song_stops_at_the_next_note_still_to_play() {
        let mut g = groups(&[0.5, 1.0, 2.0]);
        g[1].state = Some(Judgement::Hit);
        let none = |_: f64| false;
        let all = (f64::NEG_INFINITY, f64::INFINITY);
        assert_eq!(hold_at(&g, 0.4, all, none), None, "before the first note");
        assert_eq!(hold_at(&g, 0.48, all, none), Some(0), "just before it");
        assert_eq!(
            hold_at(&g, 1.5, all, none),
            None,
            "0.5 is long gone and 1.0 was played"
        );
        assert_eq!(hold_at(&g, 1.96, all, none), None);
        assert_eq!(hold_at(&g, 1.98, all, none), Some(2));
        assert_eq!(
            hold_at(&g, 2.1, all, none),
            Some(2),
            "a frame late still stops there"
        );
        // played in time, its pluck still being read: the song goes on until the grace runs out
        let reading = |from: f64| from <= 1.9;
        assert_eq!(hold_at(&g, 2.0, all, reading), None);
        assert_eq!(hold_at(&g, 2.1, all, reading), None);
        assert_eq!(hold_at(&g, 2.16, all, reading), Some(2));
        g[2].state = Some(Judgement::Hit);
        assert_eq!(hold_at(&g, 2.1, all, none), None, "played: no stop");
    }

    #[test]
    fn in_a_lesson_only_the_stretchs_notes_are_waited_at() {
        // the loop's lead-in plays notes before the stretch: they don't stop it, nor do notes after its end
        let g = groups(&[0.5, 1.0, 2.0, 3.0]);
        let none = |_: f64| false;
        let span = (0.95, 2.5);
        assert_eq!(hold_at(&g, 0.5, span, none), None, "in the lead-in");
        assert_eq!(hold_at(&g, 0.98, span, none), Some(1));
        assert_eq!(hold_at(&g, 1.99, span, none), Some(2));
        assert_eq!(hold_at(&g, 2.99, span, none), None, "after the stretch");
    }
}
