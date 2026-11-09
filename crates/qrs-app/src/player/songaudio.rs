//! The song's own audio in the player (P20.3, new in the Rust port): a stemmed feedpak's stem mixer (level, mute
//! and solo per stem) and guitar-input transposition (-12..+12 semitones). Both are remembered per song in userdata.json
//! (`song_audio`).
//!
//! A new stem mix is summed on a worker thread and swapped into the engine where the song is (`replace_song`).
//! Slow-down is a `StretchJob`; the pitch control shifts the guitar monitor and pitch interpretation, not the song.
//! The tab and backing track stay as written, while scoring, the tuner and takes follow the input shift.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use qrs_audio::decode::SongBuffer;
use qrs_audio::stems::{GAIN_STEP, MAX_GAIN, StemMix, StemSet};
use qrs_audio::stretch::{MAX_SEMITONES, StretchJob};
use qrs_library::userdata::{SongAudio, StemSetting};

use super::PlayerView;
use crate::state::AppState;
use crate::transport::Source;

/// A mixer change waits this long for the next before the stems are summed.
const REMIX_SETTLE: f32 = 0.12;

#[derive(Default)]
pub struct Mixer {
    /// A stemmed feedpak's stems, and how each sits in the mix.
    pub stems: Option<Arc<StemSet>>,
    pub mix: Vec<StemMix>,
    remix_at: Option<Instant>,
    remix_rx: Option<Receiver<SongBuffer>>,
    /// Semitones the guitar input is shifted by (-: down, +: up).
    pub pitch: i32,
    /// A slowed version getting ready while the old one plays on.
    pending: Option<StretchJob>,
}

impl Mixer {
    /// The stems and the saved settings of a song being opened.
    pub fn new(stems: Option<Arc<StemSet>>, saved: &SongAudio) -> Mixer {
        let mut m = Mixer {
            // Earlier builds stored the backing-track shift. Reverse its sign to preserve the same guitar/track
            // pitch relationship with the new input-shift meaning.
            pitch: -saved.pitch.clamp(-MAX_SEMITONES, MAX_SEMITONES),
            ..Default::default()
        };
        if let Some(set) = &stems {
            m.mix = set.default_mix();
            if !saved.stems.is_empty() {
                for (s, mx) in set.stems.iter().zip(m.mix.iter_mut()) {
                    if let Some(v) = saved.stems.get(&s.info.id) {
                        *mx = StemMix {
                            gain: (v.gain as f32).clamp(0.0, MAX_GAIN),
                            on: v.on,
                            solo: v.solo,
                        };
                    }
                }
                if !set.is_default(&m.mix) {
                    m.remix_at = Some(Instant::now() - std::time::Duration::from_secs(1));
                }
            }
        }
        m.stems = stems;
        m
    }

    /// What's kept in userdata.json.
    fn saved(&self) -> SongAudio {
        let mut a = SongAudio {
            // SongAudio.pitch retains the old backing-track-shift sign convention.
            pitch: -self.pitch,
            ..Default::default()
        };
        if let Some(set) = &self.stems
            && !set.is_default(&self.mix)
        {
            for (s, m) in set.stems.iter().zip(&self.mix) {
                a.stems.insert(
                    s.info.id.clone(),
                    StemSetting {
                        gain: m.gain as f64,
                        on: m.on,
                        solo: m.solo,
                    },
                );
            }
        }
        a
    }

    /// The pending version's progress, while it's being made.
    pub fn pending_progress(&self) -> Option<f64> {
        self.pending.as_ref().filter(|j| !j.done()).map(|j| j.progress())
    }

    pub fn cancel(&mut self) {
        if let Some(j) = self.pending.take() {
            j.cancel();
        }
        self.remix_at = None;
        self.remix_rx = None;
    }
}

/// "+2", "−3", "0" (the controls' value key).
pub fn pitch_short(p: i32) -> String {
    match p.signum() {
        1 => format!("+{p}"),
        -1 => format!("\u{2212}{}", -p),
        _ => "0".into(),
    }
}

/// "+2 semitones", "original pitch".
pub fn pitch_long(p: i32) -> String {
    match p.abs() {
        0 => "original pitch".into(),
        1 => format!("{} semitone", pitch_short(p)),
        _ => format!("{} semitones", pitch_short(p)),
    }
}

fn shifted_tuning_name(offsets: &[i32], is_bass: bool, semitones: i32) -> String {
    let shifted: Vec<i32> = offsets.iter().map(|&o| o + semitones).collect();
    qrs_formats::model::tuning_name(&shifted, is_bass)
}

fn chart_string_names(opens: &[i32], is_bass: bool) -> Vec<&'static str> {
    let names: Vec<&'static str> = opens.iter().map(|&m| qrs_formats::model::note_name(m)).collect();
    qrs_formats::model::string_labels(&names, is_bass)
}

impl PlayerView {
    /// The song's open strings. Input transposition does not change the track or the chart.
    pub(super) fn sounding_opens(&self) -> Vec<i32> {
        self.chart.opens.clone()
    }

    /// The tuning after the selected guitar-input shift (E Standard −1: Eb Standard).
    pub(super) fn transposed_input_tuning_name(&self) -> String {
        // The transpose amount is relative to E standard, not to the song's chart tuning.
        shifted_tuning_name(&[0; 6], self.chart.arr.is_bass, self.mixer.pitch)
    }

    /// The tuning written in the chart and heard on the backing track.
    pub(super) fn sounding_tuning_name(&self) -> String {
        qrs_formats::model::tuning_name(&self.chart.arr.tuning, self.chart.arr.is_bass)
    }

    /// The chart's open strings; input transpose changes the reading, not the tuner targets.
    pub(super) fn chart_string_names(&self) -> Vec<&'static str> {
        chart_string_names(&self.chart.arr.open_string_midi(), self.chart.arr.is_bass)
    }

    /// A fresh scorer for the chart, compensating for the guitar input's pitch shift.
    pub(super) fn new_scorer(&self) -> qrs_analysis::Scorer {
        let mut s = qrs_analysis::Scorer::new(&self.chart.arr);
        s.transpose(-self.mixer.pitch);
        s
    }

    fn save_song_audio(&self, st: &mut AppState) {
        st.userdata.set_song_audio(&self.sid, self.mixer.saved());
    }

    /// The input pitch keys: up or down a semitone (0: no shift).
    pub(super) fn change_pitch(&mut self, st: &mut AppState, step: i32) {
        let Some(engine) = self.engine().cloned() else {
            self.say("Input transpose needs the audio engine", "");
            return;
        };
        let new = if step == 0 {
            0
        } else {
            (self.mixer.pitch + step).clamp(-MAX_SEMITONES, MAX_SEMITONES)
        };
        if new == self.mixer.pitch {
            if step != 0 {
                self.say(
                    format!(
                        "Already {} semitones {}",
                        MAX_SEMITONES,
                        if step > 0 { "up" } else { "down" }
                    ),
                    "",
                );
            }
            return;
        }
        self.mixer.pitch = new;
        engine.set_input_transpose(new as f32);
        self.tuner_hist.clear();
        self.tuner_midi = None;
        self.scorer.transpose(-new);
        self.save_song_audio(st);
        self.say(format!("Guitar input {}", pitch_long(new)), "");
    }

    /// The stem mixer's keys: `stem:on:i`, `stem:solo:i`, `stem:up:i`, `stem:down:i`, `stem:reset`.
    pub(super) fn stem_action(&mut self, st: &mut AppState, what: &str) {
        let Some(set) = self.mixer.stems.clone() else {
            return;
        };
        let mut it = what.split(':');
        let (kind, i) = (
            it.next().unwrap_or(""),
            it.next().and_then(|s| s.parse::<usize>().ok()),
        );
        let say = match (kind, i) {
            ("reset", _) => {
                self.mixer.mix = set.default_mix();
                "Stems as the pack sets them".to_string()
            }
            (_, Some(i)) if i < self.mixer.mix.len() => {
                let name = set.stems[i].info.name.clone();
                let m = &mut self.mixer.mix[i];
                match kind {
                    "on" => {
                        m.on = !m.on;
                        format!("{name} {}", if m.on { "on" } else { "muted" })
                    }
                    "solo" => {
                        m.solo = !m.solo;
                        format!("{name} solo {}", if m.solo { "on" } else { "off" })
                    }
                    "set" => {
                        // the controls' level slider: stem:set:<i>:<gain>
                        let Some(g) = it.next().and_then(|s| s.parse::<f32>().ok()) else {
                            return;
                        };
                        let g = (g * 10.0).round().clamp(0.0, MAX_GAIN * 10.0) / 10.0;
                        if (g - m.gain).abs() < 1e-4 {
                            return;
                        }
                        m.gain = g;
                        format!("{name} {}%", (m.gain * 100.0).round() as i64)
                    }
                    "up" | "down" => {
                        let d = if kind == "up" { GAIN_STEP } else { -GAIN_STEP };
                        m.gain = ((m.gain + d) * 10.0).round().clamp(0.0, MAX_GAIN * 10.0) / 10.0;
                        format!("{name} {}%", (m.gain * 100.0).round() as i64)
                    }
                    _ => return,
                }
            }
            _ => return,
        };
        self.mixer.remix_at = Some(Instant::now());
        self.save_song_audio(st);
        self.say(say, "");
    }

    /// Start a slowed version of the song for `speed` (the original at normal speed). With
    /// `immediate` (or a new speed) it plays at once from where it's ready; otherwise the version playing keeps on
    /// until the new one is ready at the playhead.
    pub(super) fn restretch(&mut self, speed: f64, immediate: bool) {
        let Source::Engine { e, song } = &self.transport.source else {
            return;
        };
        let (e, song) = (e.clone(), song.clone());
        if let Some(j) = self.mixer.pending.take() {
            j.cancel();
        }
        if (speed - 1.0).abs() < 1e-6 {
            if let Some(job) = self.stretch.take() {
                job.cancel();
            }
            e.set_speed(1.0, None);
            return;
        }
        let job = StretchJob::start_shifted(song, speed, 0.0, e.position());
        if immediate || (e.speed() - speed).abs() > 1e-6 {
            if let Some(old) = self.stretch.take() {
                old.cancel();
            }
            e.set_speed(speed, Some(job.buffer.clone()));
            self.stretch = Some(job);
        } else {
            self.mixer.pending = Some(job);
        }
    }

    /// Put `buf` in the engine where the song is (the playhead stays), and make the slowed or shifted version from
    /// it if one is playing.
    pub(super) fn swap_song(&mut self, buf: SongBuffer) {
        let Some(e) = self.engine().cloned() else { return };
        let playing = self.transport.playing();
        e.replace_song(buf.clone());
        if let Source::Engine { song, .. } = &mut self.transport.source {
            *song = Arc::new(buf);
        }
        if self.stretch.is_some() || self.mixer.pending.is_some() {
            self.restretch(e.speed(), !playing);
        }
    }

    /// Mix the stems as the mixer has them now (the synth guitar going off).
    pub(super) fn remix_now(&mut self) {
        self.mixer.remix_at = Some(Instant::now() - std::time::Duration::from_secs(1));
    }

    /// Every frame: a settled mixer change starts its mix; a finished mix goes in, and a slowed version takes over
    /// when ready.
    pub(super) fn update_song_audio(&mut self) {
        let Some(e) = self.engine().cloned() else { return };
        if self.mixer.remix_rx.is_none()
            && let Some(set) = self.mixer.stems.clone()
            && self
                .mixer
                .remix_at
                .is_some_and(|t| t.elapsed().as_secs_f32() > REMIX_SETTLE)
        {
            self.mixer.remix_at = None;
            // (with the synth guitar on, its version is mixed again instead: songsynth.rs)
            if !self.song_synth_remix() {
                let (tx, rx) = channel();
                let mix = self.mixer.mix.clone();
                let spawned = std::thread::Builder::new()
                    .name("qrs-stem-mix".into())
                    .spawn(move || {
                        let _ = tx.send(set.mix(&mix));
                    });
                if spawned.is_ok() {
                    self.mixer.remix_rx = Some(rx);
                }
            }
        }
        if let Some(rx) = &self.mixer.remix_rx {
            match rx.try_recv() {
                Ok(buf) => {
                    self.mixer.remix_rx = None;
                    if !self.song_synth_remix() {
                        self.swap_song(buf);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.mixer.remix_rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(j) = &self.mixer.pending
            && (j.done() || j.ready_at(e.position()))
        {
            let j = self.mixer.pending.take().unwrap();
            if let Some(old) = self.stretch.take() {
                old.cancel();
            }
            e.set_speed(j.speed, Some(j.buffer.clone()));
            self.stretch = Some(j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_labels() {
        assert_eq!(pitch_short(2), "+2");
        assert_eq!(pitch_short(-3), "\u{2212}3");
        assert_eq!(pitch_long(0), "original pitch");
        assert_eq!(pitch_long(-1), "\u{2212}1 semitone");
        assert_eq!(pitch_long(12), "+12 semitones");
    }

    #[test]
    fn saved_pitch_migrates_to_input_shift_and_round_trips() {
        let saved = SongAudio {
            pitch: 3,
            ..Default::default()
        };
        let mixer = Mixer::new(None, &saved);
        assert_eq!(mixer.pitch, -3);
        let saved_again = mixer.saved();
        assert_eq!(saved_again.pitch, 3);
        assert_eq!(Mixer::new(None, &saved_again).pitch, -3);
    }

    #[test]
    fn standard_tuning_names_follow_the_input_transpose() {
        let e = [0; 6];
        assert_eq!(shifted_tuning_name(&e, false, 0), "E Standard");
        assert_eq!(shifted_tuning_name(&e, false, -1), "Eb Standard");
        assert_eq!(shifted_tuning_name(&e, false, -2), "D Standard");
    }

    #[test]
    fn chart_string_labels_stay_at_chart_pitch() {
        assert_eq!(
            chart_string_names(&[39, 44, 49, 54, 58, 63], false),
            ["Eb", "Ab", "C#", "F#", "Bb", "eb"]
        );
    }
}
