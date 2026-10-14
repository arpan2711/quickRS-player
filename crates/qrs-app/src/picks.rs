//! The app's pick (the gold one in the dark themes, the graphite one in Daylight), loaded once as a texture.

use std::cell::RefCell;
use std::collections::HashMap;

use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureId, TextureOptions};

const GOLD: &[u8] = include_bytes!("../../../assets/icons/pick-gold.png");
const GRAPHITE: &[u8] = include_bytes!("../../../assets/icons/pick-graphite.png");

thread_local! {
    static PICKS: RefCell<HashMap<&'static str, Option<TextureHandle>>> = RefCell::new(HashMap::new());
}

pub fn pick(ctx: &egui::Context, name: &'static str) -> Option<TextureId> {
    PICKS.with(|m| {
        let mut m = m.borrow_mut();
        m.entry(name)
            .or_insert_with(|| {
                let bytes = if name == "pick-gold" { GOLD } else { GRAPHITE };
                let img = image::load_from_memory(bytes).ok()?.to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                Some(ctx.load_texture(
                    name,
                    ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                    TextureOptions::LINEAR,
                ))
            })
            .as_ref()
            .map(|t| t.id())
    })
}
