//! Opening a song in the background: the chart, the lyrics and the song's audio (decoded at the engine's rate),
//! with the loading card meanwhile (port of rsplay/app.py `open_song` / `draw_loading`).

use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use eframe::egui;
use egui::{Align2, Painter, Rect, pos2, vec2};
use qrs_audio::decode::{SongBuffer, decode, to_song_buffer};
use qrs_formats::lyrics::Line;
use qrs_formats::{Arrangement, Keys, Package};
use qrs_library::SongEntry;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::{pixels, px};

use crate::library::{centerx, ipx, snap, trunc_px, txt};

pub struct Loaded {
    pub g: usize,
    pub entry: SongEntry,
    /// The part's key.
    pub part: String,
    pub arr: Arrangement,
    pub lyrics: Vec<Line>,
    pub song: Option<SongBuffer>,
    pub audio_error: String,
}

pub struct Loading {
    pub title: String,
    pub started: Instant,
    rx: Receiver<Result<Loaded, String>>,
    /// Screenshot mode's stand-in: the sender kept so it never finishes.
    stub: Option<std::sync::mpsc::Sender<Result<Loaded, String>>>,
}

impl Loading {
    /// Start reading row `g`'s song `e` (part `part`), decoding its audio at `rate` (None: no engine, no audio).
    pub fn start(g: usize, e: &SongEntry, part: &str, keys: &Keys, rate: Option<u32>) -> Loading {
        let (tx, rx) = channel();
        let (entry, part_key, keys) = (e.clone(), part.to_string(), keys.clone());
        let title = e.display_title();
        std::thread::Builder::new()
            .name("qrs-open-song".into())
            .spawn(move || {
                let res = (|| -> Result<Loaded, String> {
                    let sk = (!entry.song_key.is_empty()).then_some(entry.song_key.as_str());
                    let mut pkg = Package::open(&entry.path, &keys, sk).map_err(|e| e.to_string())?;
                    let arr = pkg.load(Some(&part_key), 1.0).map_err(|e| e.to_string())?;
                    // lyrics are a nice-to-have: never block the song on them
                    let lyrics = pkg.load_lyrics().unwrap_or_default();
                    let mut audio_error = String::new();
                    let song = match rate {
                        None => None,
                        Some(rate) => match qrs_formats::song_audio::song_audio(&mut pkg, false) {
                            Ok(Some(enc)) => match decode(&enc)
                                .map_err(|e| e.to_string())
                                .and_then(|pcm| to_song_buffer(pcm, rate).map_err(|e| e.to_string()))
                            {
                                Ok(b) => Some(b),
                                Err(e) => {
                                    audio_error = e;
                                    None
                                }
                            },
                            Ok(None) => {
                                audio_error = "the song has no audio".into();
                                None
                            }
                            Err(e) => {
                                audio_error = e.to_string();
                                None
                            }
                        },
                    };
                    Ok(Loaded {
                        g,
                        entry,
                        part: part_key,
                        arr,
                        lyrics,
                        song,
                        audio_error,
                    })
                })();
                let _ = tx.send(res);
            })
            .ok();
        Loading {
            title,
            started: Instant::now(),
            rx,
            stub: None,
        }
    }

    /// A loading card that never finishes (screenshot mode).
    pub fn stub(title: &str) -> Loading {
        let (tx, rx) = channel();
        Loading {
            title: title.into(),
            started: Instant::now(),
            rx,
            stub: Some(tx),
        }
    }

    pub fn is_stub(&self) -> bool {
        self.stub.is_some()
    }

    /// The result once it's ready.
    pub fn poll(&self) -> Option<Result<Loaded, String>> {
        self.rx.try_recv().ok()
    }

    /// The song's title on a raised card, with three lights taking turns to come on (laid out in the Python app's
    /// whole pixels, as the library).
    pub fn draw(&self, p: &Painter, area: Rect) {
        let th = t();
        let ctx = p.ctx();
        let (w, h) = (ipx(area.width()), ipx(area.height()));
        let tw = text::width(ctx, &self.title, Style::new(22.0, 800, th.text)).min(trunc_px(w * 0.6));
        let (cw, ch) = (px(300.0).max(tw + px(52.0)), px(150.0));
        // card.center = (w // 2, h // 2)
        let card = Rect::from_min_size(
            area.min
                + vec2(
                    pixels((w / 2.0).floor() as f32) - snap(cw / 2.0),
                    pixels((h / 2.0).floor() as f32) - snap(ch / 2.0),
                ),
            vec2(cw, ch),
        );
        paint::raised(p, card, Some(26.0), 8.0, Fill::Base, 0.0);
        let mid = centerx(card);
        txt(
            p,
            &self.title,
            pos2(mid, card.min.y + px(26.0)),
            22.0,
            th.text,
            800,
            Align2::CENTER_TOP,
            Some(card.width() - px(52.0)),
        );
        let lit = (self.started.elapsed().as_secs_f32() * 3.0) as usize % 3;
        for i in 0..3 {
            let c = pos2(mid - px(24.0) + i as f32 * px(24.0), card.min.y + px(80.0));
            if i == lit {
                paint::led(p, c, 12.0, th.accent);
            } else {
                paint::dot(p, c, 6.0, th.unlit);
            }
        }
        txt(
            p,
            "Loading…",
            pos2(mid, card.max.y - px(24.0)),
            14.0,
            th.caption,
            500,
            Align2::CENTER_BOTTOM,
            None,
        );
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
    }
}
