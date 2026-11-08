//! The song's own audio in the player (P20.3, new in the Rust port): a stemmed feedpak's stem mixer (level, mute
//! and solo per stem) and pitch shift (-12..+12 semitones, the speed unchanged). Both are remembered per song in
//! userdata.json (`song_audio`).
//!
//! A new stem mix is summed on a worker thread and swapped into the engine where the song is (`replace_song`). A
//! pitch shift (and slow-down, which may come with it) is a `StretchJob` like the slow-down's; when only the mix or
//! the pitch changed, the version playing keeps playing until the new one is ready at the playhead, so there's no
//! gap. The tab stays as written; the scorer, the tuner's "this song" tuning and the selection synth follow the
//! shift (DECISIONS D38).

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

/// Taps closer together than this build one version (as the speed keys).
const SETTLE: f32 = 0.45;
/// A mixer change waits this long for the next before the stems are summed.
const REMIX_SETTLE: f32 = 0.12;

#[derive(Default)]
pub struct Mixer {
    /// A stemmed feedpak's stems, and how each sits in the mix.
    pub stems: Option<Arc<StemSet>>,
    pub mix: Vec<StemMix>,
    remix_at: Option<Instant>,
    remix_rx: Option<Receiver<SongBuffer>>,
    /// Semitones the song is shifted by.
    pub pitch: i32,
    pitch_at: Option<Instant>,
    /// A new version (slowed and/or shifted) getting ready while the old one plays on.
    pending: Option<StretchJob>,
}

impl Mixer {
    /// The stems and the saved settings of a song being opened.
    pub fn new(stems: Option<Arc<StemSet>>, saved: &SongAudio) -> Mixer {
        let mut m = Mixer {
            pitch: saved.pitch.clamp(-MAX_SEMITONES, MAX_SEMITONES),
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
            pitch: self.pitch,
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

impl PlayerView {
    /// The open strings as they sound with the pitch shift (what the synth plays and the tuner aims at).
    pub(super) fn sounding_opens(&self) -> Vec<i32> {
        self.chart.opens.iter().map(|o| o + self.mixer.pitch).collect()
    }

    /// The tuning the song sounds in with the pitch shift ("E Standard" for Eb Standard shifted up one).
    pub(super) fn sounding_tuning_name(&self) -> String {
        let offs: Vec<i32> = self
            .chart
            .arr
            .tuning
            .iter()
            .map(|o| o + self.mixer.pitch)
            .collect();
        qrs_formats::model::tuning_name(&offs, self.chart.arr.is_bass)
    }

    /// The open strings' names with the pitch shift.
    pub(super) fn sounding_string_names(&self) -> Vec<&'static str> {
        let names: Vec<&'static str> = self
            .chart
            .arr
            .open_string_midi()
            .into_iter()
            .map(|m| qrs_formats::model::note_name(m + self.mixer.pitch))
            .collect();
        qrs_formats::model::string_labels(&names, self.chart.arr.is_bass)
    }

    /// A fresh scorer for the chart, expecting the shifted pitch.
    pub(super) fn new_scorer(&self) -> qrs_analysis::Scorer {
        let mut s = qrs_analysis::Scorer::new(&self.chart.arr);
        s.transpose(self.mixer.pitch);
        s
    }

    fn save_song_audio(&self, st: &mut AppState) {
        st.userdata.set_song_audio(&self.sid, self.mixer.saved());
    }

    /// The pitch keys: up or down a semitone (0: back to the original).
    pub(super) fn change_pitch(&mut self, st: &mut AppState, step: i32) {
        if self.engine().is_none() {
            self.say("Pitch shift needs the audio engine", "");
            return;
        }
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
        self.mixer.pitch_at = Some(Instant::now());
        self.scorer.transpose(new);
        self.save_song_audio(st);
        self.say(format!("Pitch {}", pitch_long(new)), "");
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

    /// Start a new version of the song for `speed` and the pitch shift (the original when neither applies). With
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
        if (speed - 1.0).abs() < 1e-6 && self.mixer.pitch == 0 {
            if let Some(job) = self.stretch.take() {
                job.cancel();
            }
            e.set_speed(1.0, None);
            return;
        }
        let job = StretchJob::start_shifted(song, speed, self.mixer.pitch as f32, e.position());
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

    /// Every frame: a settled pitch change starts its version, a settled mixer change its mix; a finished mix goes
    /// in; a version that's ready takes over.
    pub(super) fn update_song_audio(&mut self) {
        let Some(e) = self.engine().cloned() else { return };
        let playing = self.transport.playing();
        if self
            .mixer
            .pitch_at
            .is_some_and(|t| t.elapsed().as_secs_f32() > SETTLE)
        {
            self.mixer.pitch_at = None;
            if self.pending_speed.is_none() {
                // (a speed change waiting too makes its own version, with the pitch)
                self.restretch(e.speed(), !playing);
            }
        }
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
}
