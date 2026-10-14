//! The page under a floating panel (settings, the results card), softened and dimmed (port of neu.backdrop and
//! app.py `draw_backdrop`). The page is drawn once without the panel and captured (eframe's screenshot command),
//! shrunk to a quarter with a light blur, and drawn stretched back over the window under the panel from then on,
//! until the window size or the theme changes. eframe captures the frame after the one that asks, so the page is
//! drawn alone until the capture is in (two frames); drawn dimmed meanwhile, it was dimmed twice.

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{
    Color32, ColorImage, Event, Painter, Rect, TextureHandle, TextureOptions, UserData, ViewportCommand,
};
use qrs_ui::{paint, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    w: u32,
    h: u32,
    theme: usize,
}

/// The screenshot's tag (screenshot mode saves only its own captures).
#[derive(Clone, Copy, Debug)]
pub struct BackdropTag;

#[derive(Default)]
pub struct Backdrop {
    tex: Option<TextureHandle>,
    key: Option<Key>,
    /// The capture asked for, and when (one that never comes back is given up after a second).
    pending: Option<(Key, Instant)>,
}

impl Backdrop {
    fn key_now(ctx: &egui::Context) -> Key {
        let s = ctx.content_rect().size() * ctx.pixels_per_point();
        Key {
            w: s.x.round() as u32,
            h: s.y.round() as u32,
            theme: theme::generation(),
        }
    }

    /// Call each frame a panel is up, before drawing: takes in a capture that arrived. True when the backdrop can
    /// be drawn; otherwise draw the page and, if `capturing()` is false, call `capture` (without the panel).
    pub fn ready(&mut self, ctx: &egui::Context) -> bool {
        let got = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                Event::Screenshot { user_data, image, .. }
                    if user_data
                        .data
                        .as_ref()
                        .is_some_and(|d| d.downcast_ref::<BackdropTag>().is_some()) =>
                {
                    Some(image.clone())
                }
                _ => None,
            })
        });
        let now = Self::key_now(ctx);
        if self
            .pending
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(1))
        {
            self.pending = None;
        }
        if let Some(img) = got {
            if self.pending.is_some_and(|(k, _)| k == now) {
                let small = soften(&img);
                match &mut self.tex {
                    Some(t) => t.set(small, TextureOptions::LINEAR),
                    None => self.tex = Some(ctx.load_texture("backdrop", small, TextureOptions::LINEAR)),
                }
                self.key = Some(now);
            }
            self.pending = None;
        }
        if self.pending.is_some() {
            ctx.request_repaint();
        }
        self.key == Some(now) && self.tex.is_some()
    }

    /// A capture was asked for and hasn't come back yet.
    pub fn capturing(&self) -> bool {
        self.pending.is_some()
    }

    /// Capture this frame (draw the page, not the panel).
    pub fn capture(&mut self, ctx: &egui::Context) {
        self.pending = Some((Self::key_now(ctx), Instant::now()));
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(BackdropTag)));
        ctx.request_repaint();
    }

    pub fn draw(&self, p: &Painter, rect: Rect) {
        if let Some(t) = &self.tex {
            paint::image(p, t.id(), rect, Color32::WHITE);
        }
        paint::dim(p, rect, None);
    }

    pub fn clear(&mut self) {
        self.key = None;
        self.pending = None;
    }
}

/// A quarter of the size (each pixel the mean of 4 x 4), then a 3 x 3 box blur (pygame's smoothscale down and
/// box_blur(1)); the GPU scales it back up with linear filtering.
pub fn soften(img: &ColorImage) -> ColorImage {
    let [w, h] = img.size;
    let (sw, sh) = ((w / 4).max(1), (h / 4).max(1));
    let mut small = vec![[0f32; 3]; sw * sh];
    for y in 0..sh {
        for x in 0..sw {
            let mut acc = [0f32; 3];
            let mut n = 0.0;
            for yy in y * 4..((y + 1) * 4).min(h) {
                for xx in x * 4..((x + 1) * 4).min(w) {
                    let c = img.pixels[yy * w + xx];
                    acc[0] += c.r() as f32;
                    acc[1] += c.g() as f32;
                    acc[2] += c.b() as f32;
                    n += 1.0;
                }
            }
            if n > 0.0 {
                small[y * sw + x] = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        }
    }
    let mut out = Vec::with_capacity(sw * sh);
    for y in 0..sh {
        for x in 0..sw {
            let mut acc = [0f32; 3];
            let mut n = 0.0;
            for yy in y.saturating_sub(1)..(y + 2).min(sh) {
                for xx in x.saturating_sub(1)..(x + 2).min(sw) {
                    let c = small[yy * sw + xx];
                    acc[0] += c[0];
                    acc[1] += c[1];
                    acc[2] += c[2];
                    n += 1.0;
                }
            }
            out.push(Color32::from_rgb(
                (acc[0] / n).round() as u8,
                (acc[1] / n).round() as u8,
                (acc[2] / n).round() as u8,
            ));
        }
    }
    ColorImage::new([sw, sh], out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soften_averages_and_shrinks() {
        let (w, h) = (16, 8);
        let mut px = vec![Color32::BLACK; w * h];
        for y in 0..h {
            for x in 8..w {
                px[y * w + x] = Color32::WHITE;
            }
        }
        let s = soften(&ColorImage::new([w, h], px));
        assert_eq!(s.size, [4, 2]);
        // the edge between black and white is blurred, the far columns keep their colour
        assert_eq!(s.pixels[0], Color32::BLACK);
        assert_eq!(s.pixels[3], Color32::WHITE);
        assert!(s.pixels[1].r() > 0 && s.pixels[2].r() < 255);
    }
}
