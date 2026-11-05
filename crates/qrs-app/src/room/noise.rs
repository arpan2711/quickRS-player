//! "Learn noise" (P41.1, D163; the guitar tech's #3): two and a half seconds of the input with the hands off the
//! strings, its median level kept as the input's noise floor (`calibrate::learn_noise`, the same `floor_db` Calibrate
//! measures), and the sound's gate set just over it (`board::gate_threshold`: the floor + 8 dB). New code. The strip
//! draws the action (`strip.rs`); the words and look are the room's to restyle.

use std::time::{Duration, Instant};

use qrs_audio::board;

use super::RoomView;
use crate::state::AppState;

/// How long it listens (the first quarter second is skipped: the click of the button).
pub const LISTEN: Duration = Duration::from_millis(2500);
const SKIP: Duration = Duration::from_millis(250);

/// The learning in progress (None: not learning).
#[derive(Default)]
pub struct LearnNoise {
    since: Option<Instant>,
    levels: Vec<f32>,
}

impl LearnNoise {
    pub fn start(&mut self) {
        self.since = Some(Instant::now());
        self.levels.clear();
    }

    /// Listening now, and how far through (0..1).
    pub fn progress(&self) -> Option<f32> {
        self.since
            .map(|t| (t.elapsed().as_secs_f32() / LISTEN.as_secs_f32()).min(1.0))
    }

    pub fn cancel(&mut self) {
        self.since = None;
        self.levels.clear();
    }
}

/// Every frame the room is open: while learning, the input's level is gathered; at the end the floor is kept, the
/// sound's gate set over it, and the room says what it heard.
pub fn update(room: &mut RoomView, st: &mut AppState) {
    let Some(t) = room.noise.since else { return };
    let el = t.elapsed();
    if el >= SKIP && el < LISTEN {
        room.noise.levels.push(room.listen.level_db);
        st.ctx.request_repaint();
        return;
    }
    if el < LISTEN {
        st.ctx.request_repaint();
        return;
    }
    let levels = std::mem::take(&mut room.noise.levels);
    room.noise.since = None;
    match crate::calibrate::learn_noise(st, &levels) {
        Some(floor) => {
            let thr = board::gate_threshold(false);
            let b = room.rack.board_now(st);
            if let Some(g) = b.blocks.iter().find(|x| x.def().id == "gate")
                && let Some(k) = g.def().knob_index("threshold")
            {
                room.rack.set_knob(st, g.uid, k, thr);
                room.rack.board_commit(st);
            }
            room.say(
                format!("Noise learnt: {floor:.0} dB"),
                format!("  \u{b7}  the gate opens at {thr:.0} dB"),
            );
        }
        None => room.say("Nothing heard to learn", "  \u{b7}  is the guitar plugged in?"),
    }
}
