//! The selected song's preview clip (the short "_preview" bank Rocksmith plays in its song list; a feedpak's
//! preview stem), decoded in the background and played over the engine with a fade-in; the newest request wins.
//! Port of rsplay/preview.py `Previewer`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use qrs_audio::Engine;
use qrs_audio::rt::Clip;
use qrs_formats::{Keys, Package};
use qrs_library::SongEntry;

pub struct Previewer {
    engine: Option<Arc<Engine>>,
    keys: Keys,
    /// The song id we're meant to be previewing.
    want: Arc<Mutex<Option<String>>>,
    pub loading: Arc<AtomicBool>,
    /// The engine's id for the clip playing (0: none yet).
    clip: Arc<AtomicU64>,
    /// One decode at a time.
    busy: Arc<Mutex<()>>,
    /// The song has no preview clip.
    pub none: Arc<Mutex<Option<String>>>,
    /// Screenshot mode: every preview "plays", held at (seconds in, length), as the Python tool's stand-in engine
    /// reports (9 of 30 s); nothing is decoded or played.
    pub held: Option<(f64, f64)>,
}

impl Previewer {
    pub fn new(engine: Option<Arc<Engine>>, keys: Keys) -> Previewer {
        Previewer {
            engine,
            keys,
            want: Arc::new(Mutex::new(None)),
            loading: Arc::new(AtomicBool::new(false)),
            clip: Arc::new(AtomicU64::new(0)),
            busy: Arc::new(Mutex::new(())),
            none: Arc::new(Mutex::new(None)),
            held: None,
        }
    }

    pub fn want(&self) -> Option<String> {
        self.want.lock().ok().and_then(|w| w.clone())
    }

    pub fn request(&self, e: &SongEntry) {
        let sid = e.id();
        if self.held.is_some() {
            if let Ok(mut w) = self.want.lock() {
                *w = Some(sid);
            }
            return;
        }
        let Some(engine) = self.engine.clone() else { return };
        if self.want().as_deref() == Some(sid.as_str()) {
            return;
        }
        if let Ok(mut w) = self.want.lock() {
            *w = Some(sid.clone());
        }
        engine.stop_preview(false);
        let (want, loading, clip, busy, none, keys) = (
            self.want.clone(),
            self.loading.clone(),
            self.clip.clone(),
            self.busy.clone(),
            self.none.clone(),
            self.keys.clone(),
        );
        let (path, song_key) = (e.path.clone(), e.song_key.clone());
        std::thread::Builder::new()
            .name("qrs-preview".into())
            .spawn(move || {
                let _one = busy.lock();
                let still = || want.lock().ok().and_then(|w| w.clone()).as_deref() == Some(sid.as_str());
                if !still() {
                    return; // the mouse has moved on
                }
                loading.store(true, Ordering::Release);
                let got = (|| -> Option<Clip> {
                    let mut pkg =
                        Package::open(&path, &keys, (!song_key.is_empty()).then_some(song_key.as_str()))
                            .ok()?;
                    let enc = qrs_formats::song_audio::song_audio(&mut pkg, true).ok()??;
                    let pcm = qrs_audio::decode::decode(&enc).ok()?;
                    let rate = pcm.rate;
                    Some(Clip {
                        rate,
                        samples: qrs_audio::decode::to_stereo(&pcm),
                    })
                })();
                loading.store(false, Ordering::Release);
                match got {
                    Some(c) if still() => {
                        clip.store(engine.play_preview(c, true, false), Ordering::Release);
                    }
                    None if still() => {
                        if let Ok(mut n) = none.lock() {
                            *n = Some(sid.clone());
                        }
                    }
                    _ => {}
                }
            })
            .ok();
    }

    pub fn stop(&self) {
        let had = self
            .want
            .lock()
            .ok()
            .map(|mut w| w.take().is_some())
            .unwrap_or(false);
        if had && let Some(e) = &self.engine {
            e.stop_preview(false);
        }
    }

    /// (seconds in, clip length) while our clip plays.
    pub fn position(&self) -> Option<(f64, f64)> {
        if let Some(h) = self.held {
            return self.want().map(|_| h);
        }
        let e = self.engine.as_ref()?;
        let id = self.clip.load(Ordering::Acquire);
        (id != 0 && e.preview_state(id) == qrs_audio::PreviewState::Playing).then(|| e.preview_position())
    }

    pub fn has_engine(&self) -> bool {
        self.engine.is_some()
    }
}
