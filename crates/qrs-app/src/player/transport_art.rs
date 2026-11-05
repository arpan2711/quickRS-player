//! The player's transport as baked art (P34.3, the owner's pick #8 of the tiles mock-ups; D87): the play key an
//! engraved obsidian gem (the pause sign glowing red from inside while playing, a gilt play sign in the dark stone
//! when stopped), back / forward as shell Go stones, the seek bar's thumb a tall button and its fill amber resin in a
//! channel. The gem's red light falls on the stones, the thumb and the resin's clear coat, fading with distance as
//! the thumb travels away. Blender renders of our own models (research folder `mockups/ui-main/tiles/reflect/`,
//! its `app_assets/manifest.json` = `assets/transport/manifest.json`), made at 6x (P34.7) and resampled once per scale
//! to the pixels they cover, so they're drawn 1:1 (sharp, and only those pixels kept on the GPU).
//!
//! The light is cheap: each lit thing is a base texture plus a "delta" texture of the gem's light over it, worked out
//! offline; at runtime the delta is drawn additively (a premultiplied colour with alpha 0 adds) with a grey weight
//! `w = (glow · I(d) / I(d_baked))^0.8`, `I(d) = 1 / (1 + (d / 120)²)`, d the distance from the gem's centre in app
//! points. The thumb blends the deltas baked at the two nearest of three distances; the resin's light is a
//! premultiplied layer drawn with the normal blend, weighted per column.
//!
//! The shared room (P34.5, D88; `transport_room`) is reflected in all of them: drawn over each base (the room pack's
//! own bases, its studio dimmed: the room supplies the reflections), under the gem's light and the « » ink. The room
//! pack has the stones', the thumb's and the gem's two steady signs in their normal state; hover and pressed lift
//! and sink them in code, over the reflect pack's sprite of that state (its shadow).

use eframe::egui::{
    self, Color32, ColorImage, Context, Mesh, Painter, Pos2, Rect, Shape, TextureHandle, TextureId,
    TextureOptions, pos2, vec2,
};
use std::sync::{Arc, Mutex};

use qrs_ui::units::px;

use super::transport_room::Room;

macro_rules! png {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!("../../../../assets/transport/", $name, ".png")).as_slice(),
        )
    };
}

/// Every texture, in `Tex` field order.
const FILES: [(&str, &[u8]); 36] = [
    png!("gem_lit_pause_normal"),
    png!("gem_lit_pause_hover"),
    png!("gem_lit_pause_pressed"),
    png!("gem_lit_play_normal"),
    png!("gem_lit_play_hover"),
    png!("gem_lit_play_pressed"),
    png!("gem_dark_pause_normal"),
    png!("gem_dark_pause_hover"),
    png!("gem_dark_pause_pressed"),
    png!("gem_dark_play_normal"),
    png!("gem_dark_play_hover"),
    png!("gem_dark_play_pressed"),
    png!("gem_glow"),
    png!("gem_glow_dark"),
    png!("key_base_normal"),
    png!("key_base_hover"),
    png!("key_base_pressed"),
    png!("key_delta"),
    png!("thumb_base_normal"),
    png!("thumb_base_hover"),
    png!("thumb_base_pressed"),
    png!("thumb_delta_139"),
    png!("thumb_delta_300"),
    png!("thumb_delta_587"),
    png!("panel_spill"),
    png!("track_left"),
    png!("track_tile"),
    png!("track_right"),
    png!("fill_left"),
    png!("fill_tile"),
    png!("fill_cut"),
    png!("fill_lead"),
    png!("fill_light_left"),
    png!("fill_light_tile"),
    png!("fill_light_cut"),
    png!("fill_light_lead"),
];

/// The room pack's bases (`assets/hall/`), in `ROOM_*` order.
const ROOM_FILES: [(&str, &[u8]); 4] = [
    (
        "gem_lit_pause",
        include_bytes!("../../../../assets/hall/gem_lit_pause.png").as_slice(),
    ),
    (
        "gem_dark_play",
        include_bytes!("../../../../assets/hall/gem_dark_play.png").as_slice(),
    ),
    (
        "stone_base",
        include_bytes!("../../../../assets/hall/stone_base.png").as_slice(),
    ),
    (
        "thumb_base",
        include_bytes!("../../../../assets/hall/thumb_base.png").as_slice(),
    ),
];
const ROOM_GEM_LIT: usize = 0;
const ROOM_GEM_DARK: usize = 1;
const ROOM_STONE: usize = 2;
const ROOM_THUMB: usize = 3;

const GEM: usize = 0; // 12: lit/dark x pause/play x normal/hover/pressed
// 12: gem_glow, the lit sign's glow on the panel (the room pack's lit sign carries its own since P34.5)
// 13: gem_glow_dark, the dark sign's (both replaced by halo since P34.5)
const KEY: usize = 14; // 3 states
const KEY_DELTA: usize = 17;
const THUMB: usize = 18; // 3 states
const THUMB_DELTA: usize = 21; // 3 distances
const SPILL: usize = 24;
const TRACK: usize = 25; // left, tile, right
const FILL: usize = 28; // left, tile, cut, lead
const FILL_LIGHT: usize = 32; // the same pieces

/// The light's falloff (app points), the weight's exponent, and the stopped gem's glow.
const D0: f32 = 120.0;
const P: f32 = 0.8;
const DARK_GLOW: f32 = 0.12;
/// The distances the keys' and the thumb's light were baked at, and the bar's reference.
const KEY_D: f32 = 69.0;
const THUMB_D: [f32; 3] = [139.0, 300.0, 587.0];
const BAR_D: f32 = 150.0;
/// Sizes at 1x (app points): the gem's, keys' and thumb's canvases; the bar's pieces (all 24 tall, the bar's centre
/// line at 12); the panel spill.
const GEM_CANVAS: f32 = 96.0;
const KEY_CANVAS: f32 = 64.0;
const THUMB_CANVAS: f32 = 40.0;
const PIECE_H: f32 = 24.0;
const SPILL_SIZE: [f32; 2] = [320.0, 100.0];
/// The bar's pieces (1x widths, where the slot start sits in them): the ends 70 wide with the slot 6 in (left) and
/// ending 60 in (right); a 64 tile; a 14 px cut window with its gap at 6; an 11 px lead with the round end at 9.
const END_W: f32 = 70.0;
const START_IN: f32 = 6.0;
const END_IN: f32 = 60.0;
const CUT_W: f32 = 14.0;
const CUT_AT: f32 = 6.0;
const LEAD_W: f32 = 11.0;
const LEAD_AT: f32 = 9.0;
/// The thumb's travel: its centre 13 points in from either end of the slot.
pub const THUMB_IN: f32 = 13.0;
/// The resin's light is weighted per column this wide (app points).
const COLUMN: f32 = 6.0;

/// A key's state, as its sprites are baked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Normal,
    Hover,
    Pressed,
}

impl State {
    pub fn of(r: &egui::Response) -> State {
        if r.is_pointer_button_down_on() {
            State::Pressed
        } else if r.hovered() {
            State::Hover
        } else {
            State::Normal
        }
    }

    fn i(self) -> usize {
        self as usize
    }

    /// The content's lift (app points): hover 1 up, pressed 1 down (baked into the sprites; the « » follow it).
    pub fn lift(self) -> f32 {
        match self {
            State::Normal => 0.0,
            State::Hover => -1.0,
            State::Pressed => 1.0,
        }
    }

    /// The room pack's bases have only their normal state: pressed is a little darker (as the reflect pack's is).
    fn tint(self) -> Color32 {
        match self {
            State::Pressed => Color32::from_gray(232),
            _ => Color32::WHITE,
        }
    }
}

/// `I(d)`: how much of the gem's light reaches d points away.
fn reach(d: f32) -> f32 {
    1.0 / (1.0 + (d / D0).powi(2))
}

/// The weight of a light baked at `baked` points, drawn at d with the gem at `glow`.
fn weight(glow: f32, d: f32, baked: f32) -> f32 {
    (glow * reach(d) / reach(baked)).max(0.0).powf(P)
}

/// The thumb's two deltas and their weights at d: the nodes around it, blended in ln d.
fn thumb_weights(glow: f32, d: f32) -> [(usize, f32); 2] {
    let d = d.clamp(THUMB_D[0], THUMB_D[2]);
    let k = if d <= THUMB_D[1] { 0 } else { 1 };
    let (a, b) = (THUMB_D[k], THUMB_D[k + 1]);
    let t = ((d / a).ln() / (b / a).ln()).clamp(0.0, 1.0);
    [
        (k, (1.0 - t) * weight(glow, d, a)),
        (k + 1, t * weight(glow, d, b)),
    ]
}

/// The gem's light while it lights up or dims, and which sign it shows: stopping dims the light over 0.35 s with
/// the pause sign still on, then the sign turns to play; playing turns the sign to pause at once, then the light comes
/// up over 0.3 s. Never both signs at once.
#[derive(Clone, Copy, Debug)]
pub struct Glow {
    /// 0 dark .. 1 lit, before easing.
    u: f32,
    pub pause: bool,
}

impl Default for Glow {
    fn default() -> Self {
        Glow { u: 0.0, pause: false }
    }
}

impl Glow {
    /// One frame `dt` seconds on; `playing` is what the key should show. True while it's still changing.
    pub fn step(&mut self, playing: bool, dt: f32) -> bool {
        if playing {
            self.pause = true;
            self.u = (self.u + dt / 0.30).min(1.0);
            self.u < 1.0
        } else {
            self.u = (self.u - dt / 0.35).max(0.0);
            if self.u <= 0.0 {
                self.pause = false;
            }
            self.u > 0.0 || self.pause
        }
    }

    /// How lit the gem looks (0..1, smoothstepped).
    pub fn lit(&self) -> f32 {
        self.u * self.u * (3.0 - 2.0 * self.u)
    }

    /// The light it throws: a lit gem's 1, the dark one's faint glow.
    pub fn glow(&self) -> f32 {
        let l = self.lit();
        l + (1.0 - l) * DARK_GLOW
    }
}

/// The packs' pixels a point (P34.7: 6x), and the one piece made at 1x.
const PACK_K: f32 = 6.0;
const ONE_X: &str = "panel_spill";
/// How many scales' textures are kept on the GPU (the current one and the last, for a resize back and forth).
const KEEP_SCALES: usize = 2;

/// The textures at one scale: each pack image resampled to the pixels it covers on screen.
struct Scaled {
    tex: Vec<TextureHandle>,
    room_tex: Vec<TextureHandle>,
    /// The bar's channel (left, tile, right) without its colour, for Graphite: the art's channel is Midnight navy
    /// (P37.4b).
    grey_track: Vec<TextureHandle>,
    /// The bar's tile step (points): the tile texture's whole pixels over ppp, so the tiles don't drift.
    tile_step: f32,
}

impl Scaled {
    fn id(&self, i: usize) -> TextureId {
        self.tex[i].id()
    }

    fn room_id(&self, i: usize) -> TextureId {
        self.room_tex[i].id()
    }
}

/// The cache key of a scale: the context's pixels a point and the app's layout scale (`px`).
type ScaleKey = (u32, u64);

pub struct Art {
    /// Per scale, newest last.
    scaled: Mutex<Vec<(ScaleKey, Arc<Scaled>)>>,
    room: Room,
}

/// A pack image ready to resample: premultiplied, whether it adds light (alpha 0 everywhere), its pixels a point.
fn prepared(name: &str, bytes: &[u8], room: bool) -> (image::RgbaImage, bool, f32) {
    let mut im = image::load_from_memory(bytes)
        .expect("transport art asset")
        .to_rgba8();
    let k = if name == ONE_X { 1.0 } else { PACK_K };
    if !room {
        // (premultiplied as made: an additive layer's alpha is 0 and its colour must stay)
        let additive = im.pixels().all(|p| p[3] == 0);
        return (im, additive, k);
    }
    // the room pack's bases are straight alpha; the gem alone: its baked glow (an ellipse under it) and shadow go;
    // `halo` lights the panel round it
    let gem = name.starts_with("gem_");
    let gk = im.width() as f32 / GEM_CANVAS;
    for (x, y, px) in im.enumerate_pixels_mut() {
        if gem {
            let d =
                ((x as f32 + 0.5) / gk - GEM_CANVAS / 2.0).hypot((y as f32 + 0.5) / gk - GEM_CANVAS / 2.0);
            let cov = ((GEM_DISC - d) * gk + 0.5).clamp(0.0, 1.0);
            px[3] = (px[3] as f32 * cov).round() as u8;
        }
        let a = px[3] as u32;
        for c in 0..3 {
            px[c] = ((px[c] as u32 * a + 127) / 255) as u8;
        }
    }
    (im, false, k)
}

/// Its colour out (premultiplied pixels to their luma, alpha kept): Graphite's channel (P37.4b).
fn grey(im: &mut image::RgbaImage) {
    for px in im.pixels_mut() {
        let y = (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32).round() as u8;
        px[0] = y;
        px[1] = y;
        px[2] = y;
    }
}

/// The device size of an image `k` pixels a point at `ppp`: its 1x size through `px` (as it's drawn), in whole pixels.
fn device_size(w: u32, h: u32, k: f32, ppp: f32) -> [u32; 2] {
    let d = |n: u32| ((px(n as f32 / k) * ppp).round() as u32).max(1);
    [d(w), d(h)]
}

/// One image resampled to its device size and uploaded (linear, no mips: it's drawn 1:1).
fn upload(
    ctx: &Context,
    label: String,
    (im, additive, k): (image::RgbaImage, bool, f32),
    ppp: f32,
) -> TextureHandle {
    let [w, h] = device_size(im.width(), im.height(), k, ppp);
    let r = if (w, h) == (im.width(), im.height()) {
        im
    } else {
        qrs_ui::glass::resample(&im, w, h, additive)
    };
    let img = ColorImage::from_rgba_premultiplied([w as usize, h as usize], r.as_raw());
    ctx.load_texture(label, img, TextureOptions::LINEAR)
}

impl Art {
    pub fn load(ctx: &Context) -> Art {
        let art = Art {
            scaled: Mutex::new(Vec::new()),
            room: Room::load(),
        };
        art.at_scale(ctx, ctx.pixels_per_point());
        art
    }

    /// The textures for `ppp` pixels a point at the app's current layout scale, made the first time it's seen (the
    /// packs decoded and resampled once per scale, as `qrs_ui::glass` does: drawn 1:1, sharp, and only the pixels
    /// on screen kept on the GPU).
    fn at_scale(&self, ctx: &Context, ppp: f32) -> Arc<Scaled> {
        let key: ScaleKey = (ppp.to_bits(), qrs_ui::units::scale().to_bits());
        let mut cache = self.scaled.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
            let hit = cache.remove(i);
            let s = hit.1.clone();
            cache.push(hit);
            return s;
        }
        let tex: Vec<TextureHandle> = FILES
            .iter()
            .map(|(name, bytes)| {
                upload(
                    ctx,
                    format!("transport-{name}"),
                    prepared(name, bytes, false),
                    ppp,
                )
            })
            .collect();
        let room_tex = ROOM_FILES
            .iter()
            .map(|(name, bytes)| {
                upload(
                    ctx,
                    format!("transport-room-{name}"),
                    prepared(name, bytes, true),
                    ppp,
                )
            })
            .collect();
        let grey_track = FILES[TRACK..TRACK + 3]
            .iter()
            .map(|(name, bytes)| {
                let (mut im, additive, k) = prepared(name, bytes, false);
                grey(&mut im);
                upload(ctx, format!("transport-grey-{name}"), (im, additive, k), ppp)
            })
            .collect();
        let tile_step = tex[TRACK + 1].size()[0] as f32 / ppp;
        let s = Arc::new(Scaled {
            tex,
            room_tex,
            grey_track,
            tile_step,
        });
        cache.push((key, s.clone()));
        while cache.len() > KEEP_SCALES {
            cache.remove(0);
        }
        s
    }

    fn at(&self, p: &Painter) -> Arc<Scaled> {
        self.at_scale(p.ctx(), p.pixels_per_point())
    }
    /// The panel's crimson spill around the gem (draw first, before anything stands on the panel).
    pub fn spill(&self, p: &Painter, gem: Pos2, glow: &Glow) {
        let s = self.at(p);
        let size = vec2(px(SPILL_SIZE[0]), px(SPILL_SIZE[1]));
        add(
            p,
            s.id(SPILL),
            Rect::from_center_size(gem, size),
            false,
            glow.glow().powf(P),
        );
    }

    /// A key stone at its centre, `flip` for the forward key (the gem on its left), and its « » / » drawn by the
    /// caller; the stone catches the gem's light.
    pub fn key(&self, p: &Painter, centre: Pos2, state: State, flip: bool, glow: &Glow) {
        let s = self.at(p);
        let r = Rect::from_center_size(centre, vec2(px(KEY_CANVAS), px(KEY_CANVAS)));
        // the state's sprite for its shadow, the room pack's stone over it (lifted as the state is), the room in it
        image(p, s.id(KEY + state.i()), r, false, Color32::WHITE);
        let lifted = r.translate(vec2(0.0, px(state.lift())));
        image(p, s.room_id(ROOM_STONE), lifted, false, state.tint());
        self.room.stone(p, lifted, flip, KEY_CANVAS);
        add(
            p,
            s.id(KEY_DELTA),
            lifted,
            flip,
            weight(glow.glow(), KEY_D, KEY_D),
        );
    }

    /// The gem at its centre: the sign it shows, lit by `glow`, its glow on the panel under it.
    pub fn gem(&self, p: &Painter, centre: Pos2, state: State, glow: &Glow) {
        let s = self.at(p);
        let r = Rect::from_center_size(centre, vec2(px(GEM_CANVAS), px(GEM_CANVAS)));
        let lit = glow.lit();
        let lifted = r.translate(vec2(0.0, px(state.lift())));
        // the room pack's steady signs (their glow on the panel baked in): the stopped play sign, the lit pause sign
        // over it; the reflect pack's dark pause sign under the light while it comes up or dims (the room pack has
        // no such render)
        let tint = state.tint();
        // its light on the panel all round it (a little heavier below), the dark garnet's faint, under a soft contact
        // shadow
        let at = lifted.center();
        halo(
            p,
            at + vec2(0.0, px(2.5)),
            px(GEM_DISC - 2.0),
            px(GEM_DISC + 5.0),
            Color32::BLACK,
            0.35,
            0.0,
        );
        let w = glow.glow().powf(P);
        halo(
            p,
            at,
            px(GEM_DISC - 3.0),
            px(GEM_DISC + 14.0),
            HALO,
            0.19 * w,
            px(2.5),
        );
        if glow.pause {
            if lit < 1.0 {
                image(p, s.id(GEM + 6 + state.i()), r, false, Color32::WHITE);
            }
            if lit > 0.0 {
                image(
                    p,
                    s.room_id(ROOM_GEM_LIT),
                    lifted,
                    false,
                    tint.gamma_multiply(lit),
                );
            }
        } else {
            image(p, s.room_id(ROOM_GEM_DARK), lifted, false, tint);
        }
        // the room in its facets (the lit sign's table once it's half lit)
        self.room.gem(p, lifted, glow.pause && lit >= 0.5, GEM_CANVAS);
    }

    /// The seek bar: the channel from `x0` to `x1` on the centre line `cy`, the resin filled to `fill_x`, cut at
    /// each tick; the gem (at `gem`) lights its clear coat.
    #[allow(clippy::too_many_arguments)]
    pub fn bar(
        &self,
        p: &Painter,
        x0: f32,
        x1: f32,
        cy: f32,
        fill_x: f32,
        ticks: &[f32],
        gem: Pos2,
        glow: &Glow,
    ) {
        let s = self.at(p);
        let h = px(PIECE_H);
        let top = cy - h / 2.0;
        let piece = |left: f32, w: f32| Rect::from_min_size(pos2(left, top), vec2(px(w), h));
        let tile = |left: f32| Rect::from_min_size(pos2(left, top), vec2(s.tile_step, h));
        // the channel: its ends, and tiles between them (clipped to them); on Graphite without the navy
        let track = |k: usize| {
            if qrs_ui::theme::theme() == qrs_ui::theme::ThemeName::Graphite {
                s.grey_track[k].id()
            } else {
                s.id(TRACK + k)
            }
        };
        let (l, rr) = (piece(x0 - px(START_IN), END_W), piece(x1 - px(END_IN), END_W));
        let mid = Rect::from_min_max(pos2(l.max.x, top), pos2(rr.min.x, top + h));
        if mid.width() > 0.0 {
            let pc = p.with_clip_rect(mid.intersect(p.clip_rect()));
            let mut x = x0 + s.tile_step;
            while x < mid.max.x {
                image(&pc, track(1), tile(x), false, Color32::WHITE);
                x += s.tile_step;
            }
        }
        image(p, track(0), l, false, Color32::WHITE);
        image(p, track(2), rr, false, Color32::WHITE);
        // the resin, then its light: the round start, tiles from the bar's start (phase-locked) to the lead, a cut at
        // each tick under the lead, the lead's round end on the fill's end
        let lead = piece(fill_x - px(LEAD_AT), LEAD_W);
        let w_at = |x: f32| {
            let d = (pos2(x, cy) - gem).length() / px(1.0);
            weight(glow.glow(), d, BAR_D).min(1.0)
        };
        for (base, light) in [(FILL, false), (FILL_LIGHT, true)] {
            let draw = |pp: &Painter, k: usize, r: Rect| {
                if light {
                    lit_columns(pp, s.id(base + k), r, &w_at);
                } else {
                    image(pp, s.id(base + k), r, false, Color32::WHITE);
                }
            };
            let body = Rect::from_min_max(pos2(x0 - px(START_IN), top), pos2(lead.min.x + px(2.0), top + h));
            if body.width() <= 0.0 {
                continue;
            }
            // the room in the resin's clear coat, over the fill and under the gem's light
            if light {
                let cuts: Vec<f32> = ticks
                    .iter()
                    .copied()
                    .filter(|&t| t > x0 + px(4.0) && t < lead.min.x - px(4.0))
                    .collect();
                self.room.resin(p, x0, fill_x, cy, &cuts, h / PIECE_H);
            }
            let pc = p.with_clip_rect(body.intersect(p.clip_rect()));
            let start = piece(x0 - px(START_IN), END_W);
            let mut x = x0 + s.tile_step;
            while x < body.max.x {
                draw(&pc, 1, tile(x));
                x += s.tile_step;
            }
            draw(&pc, 0, start);
            for &t in ticks
                .iter()
                .filter(|&&t| t > x0 + px(4.0) && t < lead.min.x - px(4.0))
            {
                draw(&pc, 2, piece(t - px(CUT_AT), CUT_W));
            }
            draw(p, 3, lead);
        }
    }

    /// The thumb (its foot on the bar's centre line at `foot`), lit from the gem.
    pub fn thumb(&self, p: &Painter, foot: Pos2, state: State, gem: Pos2, glow: &Glow) {
        let s = self.at(p);
        let r = Rect::from_center_size(foot, vec2(px(THUMB_CANVAS), px(THUMB_CANVAS)));
        image(p, s.id(THUMB + state.i()), r, false, Color32::WHITE);
        let d = (foot - gem).length() / px(1.0);
        let lifted = r.translate(vec2(0.0, px(state.lift())));
        image(p, s.room_id(ROOM_THUMB), lifted, false, state.tint());
        self.room.thumb(p, lifted, THUMB_CANVAS);
        for (k, w) in thumb_weights(glow.glow(), d) {
            if w > 0.003 {
                add(p, s.id(THUMB_DELTA + k), lifted, false, w);
            }
        }
    }
}

/// The gem's girdle (points in its 96 canvas, with a point for its anti-aliased edge), and its light on the panel.
const GEM_DISC: f32 = 32.5;
const HALO: Color32 = Color32::from_rgb(232, 36, 64);

/// A soft round light (or shadow) on the panel: `col` at `peak` from `r0` out to nothing at `r1`, smoothly; its outer
/// rings `drop` lower than its inner ones (the light falls a little more below). Premultiplied, half additive for a
/// light, a plain blend for black.
fn halo(p: &Painter, c: Pos2, r0: f32, r1: f32, col: Color32, peak: f32, drop: f32) {
    if peak <= 0.002 || r1 <= r0 {
        return;
    }
    const RINGS: usize = 10;
    const SEG: usize = 64;
    let light = col != Color32::BLACK;
    let mut mesh = Mesh::default();
    for k in 0..=RINGS {
        let t = k as f32 / RINGS as f32;
        let r = r0 + (r1 - r0) * t;
        let a = peak * (1.0 - t * t * (3.0 - 2.0 * t));
        let pm = |v: u8| (v as f32 * a).round() as u8;
        let alpha = if light { a * 0.5 } else { a };
        let vc = Color32::from_rgba_premultiplied(
            pm(col.r()),
            pm(col.g()),
            pm(col.b()),
            (alpha * 255.0).round() as u8,
        );
        let cy = c.y + drop * t;
        for i in 0..SEG {
            let phi = std::f32::consts::TAU * i as f32 / SEG as f32;
            mesh.colored_vertex(pos2(c.x + r * phi.cos(), cy + r * phi.sin()), vc);
        }
    }
    for k in 0..RINGS as u32 {
        let (a, b) = (k * SEG as u32, (k + 1) * SEG as u32);
        for i in 0..SEG as u32 {
            let j = (i + 1) % SEG as u32;
            mesh.add_triangle(a + i, a + j, b + j);
            mesh.add_triangle(a + i, b + j, b + i);
        }
    }
    p.add(Shape::mesh(mesh));
}

/// A grey premultiplied tint: the texture at `a` of its strength.
fn grey_alpha(a: f32) -> Color32 {
    let v = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_premultiplied(v, v, v, v)
}

fn quad(mesh: &mut Mesh, r: Rect, uv: Rect, col: Color32) {
    mesh.add_rect_with_uv(r, uv, col);
}

fn uv(flip: bool) -> Rect {
    if flip {
        Rect::from_min_max(pos2(1.0, 0.0), pos2(0.0, 1.0))
    } else {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
    }
}

/// A texture over `r`, tinted (premultiplied).
fn image(p: &Painter, id: TextureId, r: Rect, flip: bool, tint: Color32) {
    let mut mesh = Mesh::with_texture(id);
    quad(&mut mesh, r, uv(flip), tint);
    p.add(Shape::mesh(mesh));
}

/// A light layer added over what's under it, at weight `w` (a premultiplied grey with alpha 0 adds).
fn add(p: &Painter, id: TextureId, r: Rect, flip: bool, w: f32) {
    if w <= 0.0 {
        return;
    }
    let v = (w.clamp(0.0, 4.0) * 255.0).round().min(255.0) as u8;
    image(p, id, r, flip, Color32::from_rgba_premultiplied(v, v, v, 0));
}

/// A premultiplied layer over `r`, its strength per column of `COLUMN` points from `w_at(x)`.
fn lit_columns(p: &Painter, id: TextureId, r: Rect, w_at: &impl Fn(f32) -> f32) {
    let n = ((r.width() / px(COLUMN)).ceil() as usize).max(1);
    let mut mesh = Mesh::with_texture(id);
    for i in 0..n {
        let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let (xa, xb) = (r.min.x + r.width() * a, r.min.x + r.width() * b);
        let col = grey_alpha(w_at((xa + xb) / 2.0));
        if col.a() == 0 {
            continue;
        }
        quad(
            &mut mesh,
            Rect::from_min_max(pos2(xa, r.min.y), pos2(xb, r.max.y)),
            Rect::from_min_max(pos2(a, 0.0), pos2(b, 1.0)),
            col,
        );
    }
    if !mesh.is_empty() {
        p.add(Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_falls_off_and_the_thumb_blends_its_nodes() {
        // at a baked distance the weight is the glow's, fading with distance
        assert!((weight(1.0, KEY_D, KEY_D) - 1.0).abs() < 1e-5);
        assert!(weight(1.0, 300.0, BAR_D) < weight(1.0, 160.0, BAR_D));
        // the thumb: one node alone at a node, two in between, nothing beyond the last but its own fade
        let at = |d: f32| thumb_weights(1.0, d);
        assert_eq!(at(139.0)[1].1, 0.0);
        let mid = at(200.0);
        assert!(mid[0].1 > 0.0 && mid[1].1 > 0.0);
        assert!(at(587.0)[0].1 < 1e-6);
        // the stopped gem still glows faintly; a lit one throws all its light
        let mut g = Glow::default();
        assert!((g.glow() - DARK_GLOW).abs() < 1e-6 && !g.pause);
        g.step(true, 1.0);
        assert!(g.pause && (g.glow() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn stopping_dims_first_then_turns_the_sign() {
        let mut g = Glow::default();
        g.step(true, 1.0);
        // stopping: the pause sign stays while it dims
        g.step(false, 0.2);
        assert!(g.pause && g.lit() > 0.0);
        g.step(false, 0.2);
        assert!(!g.pause && g.lit() == 0.0);
        // playing: the sign at once, the light after
        g.step(true, 0.01);
        assert!(g.pause && g.lit() < 0.1);
    }

    #[test]
    fn every_asset_loads_at_its_size() {
        let size = |i: usize| {
            let im = image::load_from_memory(FILES[i].1).expect(FILES[i].0);
            (im.width(), im.height())
        };
        assert_eq!(size(GEM), (576, 576));
        assert_eq!(size(KEY_DELTA), (384, 384));
        assert_eq!(size(THUMB_DELTA + 2), (240, 240));
        assert_eq!(size(TRACK + 1), (384, 144));
        assert_eq!(size(FILL + 2), (84, 144));
        assert_eq!(size(FILL_LIGHT + 3), (66, 144));
        assert_eq!(size(SPILL), (320, 100));
        // the room pack's bases: the gem's 6x canvas, the stone's and the thumb's
        let room = |i: usize| {
            let im = image::load_from_memory(ROOM_FILES[i].1).expect(ROOM_FILES[i].0);
            (im.width(), im.height())
        };
        assert_eq!(room(ROOM_GEM_LIT), (576, 576));
        assert_eq!(room(ROOM_GEM_DARK), (576, 576));
        assert_eq!(room(ROOM_STONE), (384, 384));
        assert_eq!(room(ROOM_THUMB), (240, 240));
    }

    #[test]
    fn the_textures_are_resampled_once_per_scale() {
        // (the layout scale is unset in tests: `px` passes points through)
        let ctx = Context::default();
        let art = Art::load(&ctx);
        let at = |ppp: f32| art.at_scale(&ctx, ppp);
        let a = at(2.2);
        // each texture at the pixels it covers: the 96 point gem at 2.2 is 211 px, the 64 point stone 141, the
        // 1x panel spill scaled up with them; the tile step is the tile's whole pixels
        assert_eq!(a.tex[GEM].size(), [211, 211]);
        assert_eq!(a.tex[KEY_DELTA].size(), [141, 141]);
        assert_eq!(a.tex[SPILL].size(), [704, 220]);
        assert_eq!(a.room_tex[ROOM_THUMB].size(), [88, 88]);
        assert_eq!(a.tex[TRACK + 1].size(), [141, 53]);
        assert!((a.tile_step - 141.0 / 2.2).abs() < 1e-5);
        // the same scale again is the same textures, not new ones
        assert!(Arc::ptr_eq(&a, &at(2.2)));
        assert_eq!(a.id(GEM), at(2.2).id(GEM));
        // a new scale has its own; the last two are kept, an older one goes
        let b = at(1.0);
        assert_eq!(b.tex[GEM].size(), [96, 96]);
        assert_ne!(b.id(GEM), a.id(GEM));
        assert!(Arc::ptr_eq(&a, &at(2.2)));
        at(1.5);
        assert_eq!(art.scaled.lock().unwrap().len(), KEEP_SCALES);
        assert!(!Arc::ptr_eq(&b, &at(1.0)));
        // an additive layer keeps its light through the resample (its alpha stays 0)
        let (im, additive, _) = prepared(FILES[KEY_DELTA].0, FILES[KEY_DELTA].1, false);
        assert!(additive);
        let r = qrs_ui::glass::resample(&im, 141, 141, true);
        assert!(r.pixels().all(|p| p[3] == 0) && r.pixels().any(|p| p[0] > 0));
    }
}
