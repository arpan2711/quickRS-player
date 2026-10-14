//! Album covers as egui textures: read from the PNG cache (qrs-library's ArtCache, the Python app's file names), or
//! decoded from the package in the background the first time; newest request first, so the rows on screen come
//! before ones scrolled past. Covers drawn bigger than 150 pixels use the package's 256 px texture when it has one.
//! Port of rsplay/preview.py `ArtCache`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureId, TextureOptions};
use qrs_formats::Keys;
use qrs_library::SongEntry;
use qrs_library::art::ArtCache;
use qrs_ui::raster::{Mask, pg::Surf};

/// Covers drawn bigger than this (physical pixels) come from the 256 px texture.
const BIG: f32 = 150.0;
const MAX_TEXTURES: usize = 400;
/// Pictures made for a size kept (as neu.py's `_art`).
const MAX_SIZED: usize = 160;

struct Job {
    key: String,
    sid: String,
    path: PathBuf,
    song_key: String,
    res: u32,
}

#[derive(Default)]
struct Inner {
    ready: HashMap<String, (Arc<Surf>, u64)>,
    /// (key, size, width, height, radius) -> the picture as drawn (`neu.blit_art`), and when it was last used
    sized: HashMap<(String, u32, u32, u32, u32), (TextureHandle, u64)>,
    missing: HashSet<String>,
    queued: HashSet<String>,
    queue: VecDeque<Job>,
    tick: u64,
}

pub struct Covers {
    inner: Arc<(Mutex<Inner>, Condvar)>,
}

impl Covers {
    pub fn new(ctx: egui::Context, dir: PathBuf, keys: Keys) -> Covers {
        let inner: Arc<(Mutex<Inner>, Condvar)> = Arc::new((Mutex::new(Inner::default()), Condvar::new()));
        for n in 0..2 {
            let inner = inner.clone();
            let ctx = ctx.clone();
            let cache = ArtCache::new(dir.clone());
            let keys = keys.clone();
            std::thread::Builder::new()
                .name(format!("qrs-covers-{n}"))
                .spawn(move || work(inner, ctx, cache, keys))
                .ok();
        }
        Covers { inner }
    }

    /// The cover of `e` for drawing `size_px` physical pixels big, or None while it's being read (or if there is
    /// none).
    /// The cover of `e` scaled to `size` pixels square (`ArtCache.get`) and drawn in a `w` x `h` box with corners
    /// of radius `r` (`neu.blit_art`: smoothscaled again to the box, its corners cut anti-aliased), as a texture
    /// to draw one texel to a pixel; None while it's being read (or if there is none).
    pub fn art(
        &self,
        ctx: &egui::Context,
        e: &SongEntry,
        size: u32,
        w: u32,
        h: u32,
        r: u32,
    ) -> Option<TextureId> {
        let sid = e.id();
        let (key, base) = match (size as f32 > BIG)
            .then(|| format!("{sid}@256"))
            .and_then(|k| self.lookup(&k, e, 256).map(|b| (k, b)))
        {
            Some(kb) => kb,
            None => (sid.clone(), self.lookup(&sid, e, 128)?),
        };
        let (m, _) = &*self.inner;
        let mut g = m.lock().ok()?;
        let tick = g.tick;
        let k = (key, size, w, h, r);
        if let Some((t, used)) = g.sized.get_mut(&k) {
            *used = tick;
            return Some(t.id());
        }
        let s = base
            .smoothscale(size as i32, size as i32)
            .smoothscale(w as i32, h as i32);
        let mask = Mask::rrect_full(w as usize, h as usize, r as f32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for (i, px) in s.px.iter().enumerate() {
            let a = (mask.at(i % w as usize, i / w as usize) * 255.0 + 0.5) as u8;
            rgba.extend_from_slice(&[px[0], px[1], px[2], px[3].min(a)]);
        }
        let tex = ctx.load_texture(
            format!("cover-{k:?}"),
            ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
            TextureOptions::NEAREST,
        );
        let id = tex.id();
        g.sized.insert(k, (tex, tick));
        if g.sized.len() > MAX_SIZED
            && let Some(old) = g
                .sized
                .iter()
                .min_by_key(|(_, (_, u))| *u)
                .map(|(k, _)| k.clone())
        {
            g.sized.remove(&old);
        }
        Some(id)
    }

    fn lookup(&self, key: &str, e: &SongEntry, res: u32) -> Option<Arc<Surf>> {
        let (m, cv) = &*self.inner;
        let mut g = m.lock().ok()?;
        g.tick += 1;
        let tick = g.tick;
        if let Some((t, used)) = g.ready.get_mut(key) {
            *used = tick;
            return Some(t.clone());
        }
        if g.missing.contains(key) || g.queued.contains(key) {
            return None;
        }
        g.queued.insert(key.to_string());
        g.queue.push_front(Job {
            key: key.to_string(),
            sid: e.id(),
            path: PathBuf::from(&e.path),
            song_key: e.song_key.clone(),
            res,
        });
        cv.notify_one();
        None
    }
}

/// `neu.blit_art(screen, covers.get(song, size), rect, r, frame)`: the frame, and `e`'s cover in it once it's read.
/// Whether the cover was there to draw.
pub fn draw(
    p: &egui::Painter,
    covers: &Covers,
    e: &SongEntry,
    size: f64,
    rect: egui::Rect,
    r: f32,
    frame: bool,
) -> bool {
    let mut shown = false;
    qrs_ui::paint::art_with(p, rect, r, frame, |w, h, rr| {
        let t = covers.art(p.ctx(), e, size.max(1.0) as u32, w, h, rr);
        shown = t.is_some();
        t
    });
    shown
}

fn work(inner: Arc<(Mutex<Inner>, Condvar)>, ctx: egui::Context, cache: ArtCache, keys: Keys) {
    let (m, cv) = &*inner;
    loop {
        let job = {
            let Ok(mut g) = m.lock() else { return };
            loop {
                if let Some(j) = g.queue.pop_front() {
                    break j;
                }
                g = match cv.wait(g) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
        };
        let tex = cache
            .ensure(&job.sid, &job.path, &job.song_key, &keys, job.res)
            .and_then(|png| image::open(&png).ok())
            .map(|img| {
                // (pygame's convert(): opaque)
                let rgba = img.to_rgb8();
                let (w, h) = (rgba.width() as i32, rgba.height() as i32);
                Arc::new(Surf {
                    w,
                    h,
                    px: rgba.pixels().map(|p| [p[0], p[1], p[2], 255]).collect(),
                })
            });
        let Ok(mut g) = m.lock() else { return };
        g.queued.remove(&job.key);
        match tex {
            Some(t) => {
                let tick = g.tick;
                g.ready.insert(job.key, (t, tick));
                if g.ready.len() > MAX_TEXTURES
                    && let Some(old) = g
                        .ready
                        .iter()
                        .min_by_key(|(_, (_, used))| *used)
                        .map(|(k, _)| k.clone())
                {
                    g.ready.remove(&old);
                }
                ctx.request_repaint();
            }
            None => {
                g.missing.insert(job.key);
            }
        }
    }
}
