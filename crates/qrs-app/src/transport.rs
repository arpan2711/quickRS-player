//! What the player plays on: the audio engine with the song loaded, or a silent clock (no engine, or the song's
//! audio couldn't be read) that runs the chart on its own. Port of the Python app's `audio` object (the engine or
//! `SilentClock`).

use std::sync::Arc;
use std::time::Instant;

use qrs_audio::Engine;
use qrs_audio::decode::SongBuffer;

pub struct SilentClock {
    pub duration: f64,
    started: Option<(Instant, f64)>,
    pos: f64,
    pub volume: f32,
}

impl SilentClock {
    pub fn new(duration: f64) -> SilentClock {
        SilentClock {
            duration,
            started: None,
            pos: 0.0,
            volume: 0.8,
        }
    }

    fn now(&self) -> f64 {
        match self.started {
            Some((t0, p0)) => (p0 + t0.elapsed().as_secs_f64()).min(self.duration),
            None => self.pos,
        }
    }
}

pub enum Source {
    Engine { e: Arc<Engine>, song: Arc<SongBuffer> },
    Silent(SilentClock),
}

pub struct Transport {
    pub source: Source,
    /// Screenshot mode only: the song held at this time, playing or paused, so nothing moves and a shot is
    /// exact (tools/parity/py_shots.py holds the Python app's song the same way). Play, pause and seek still
    /// reach the engine; only the clock the player reads stands still.
    held: Option<(f64, bool)>,
}

impl Transport {
    pub fn new(source: Source) -> Transport {
        Transport { source, held: None }
    }

    /// Hold the song at `at` seconds (`None`: let it run again), keeping whether it's playing.
    pub fn hold(&mut self, at: Option<f64>) {
        self.held = None;
        let playing = self.playing();
        self.held = at.map(|t| (t.clamp(0.0, self.duration()), playing));
    }

    pub fn engine(&self) -> Option<&Arc<Engine>> {
        match &self.source {
            Source::Engine { e, .. } => Some(e),
            Source::Silent(_) => None,
        }
    }

    pub fn song(&self) -> Option<&Arc<SongBuffer>> {
        match &self.source {
            Source::Engine { song, .. } => Some(song),
            Source::Silent(_) => None,
        }
    }

    /// Song seconds heard now (never goes backwards while playing).
    pub fn position(&self) -> f64 {
        if let Some((t, _)) = self.held {
            return t;
        }
        match &self.source {
            Source::Engine { e, .. } => e.position(),
            Source::Silent(c) => c.now(),
        }
    }

    pub fn duration(&self) -> f64 {
        match &self.source {
            Source::Engine { e, .. } => e.duration(),
            Source::Silent(c) => c.duration,
        }
    }

    pub fn playing(&self) -> bool {
        if let Some((_, playing)) = self.held {
            return playing;
        }
        match &self.source {
            Source::Engine { e, .. } => e.playing(),
            Source::Silent(c) => c.started.is_some() && c.now() < c.duration,
        }
    }

    pub fn finished(&self) -> bool {
        if let Some((t, _)) = self.held {
            return t >= self.duration() - 1e-3;
        }
        match &self.source {
            Source::Engine { e, .. } => e.finished(),
            Source::Silent(c) => c.now() >= c.duration - 1e-3,
        }
    }

    pub fn play(&mut self) {
        if let Some(h) = &mut self.held {
            h.1 = true;
        }
        match &mut self.source {
            Source::Engine { e, .. } => e.play(None),
            Source::Silent(c) => {
                if c.started.is_none() {
                    c.started = Some((Instant::now(), c.pos));
                }
            }
        }
    }

    pub fn pause(&mut self) {
        if let Some(h) = &mut self.held {
            h.1 = false;
        }
        match &mut self.source {
            Source::Engine { e, .. } => e.pause(),
            Source::Silent(c) => {
                c.pos = c.now();
                c.started = None;
            }
        }
    }

    pub fn toggle(&mut self) {
        if self.playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&mut self, t: f64) {
        let d = self.duration();
        if let Some(h) = &mut self.held {
            h.0 = t.clamp(0.0, d);
        }
        match &mut self.source {
            Source::Engine { e, .. } => e.seek(t),
            Source::Silent(c) => {
                let t = t.clamp(0.0, c.duration);
                c.pos = t;
                if c.started.is_some() {
                    c.started = Some((Instant::now(), t));
                }
            }
        }
    }

    pub fn volume(&self) -> f32 {
        match &self.source {
            Source::Engine { e, .. } => e.song_gain(),
            Source::Silent(c) => c.volume,
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        match &mut self.source {
            Source::Engine { e, .. } => e.set_song_gain(v.clamp(0.0, 1.0)),
            Source::Silent(c) => c.volume = v.clamp(0.0, 1.0),
        }
    }

    pub fn speed(&self) -> f64 {
        match &self.source {
            Source::Engine { e, .. } => e.speed(),
            Source::Silent(_) => 1.0,
        }
    }
}
