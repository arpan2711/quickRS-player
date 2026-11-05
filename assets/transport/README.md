# Transport pack, HQ (6x)

This is a drop-in for `quickRS-rust/assets/transport/`. It has the same 36 file names and the same manifest
structure, with `"scale": 6` added at the top. Every `files[*].px_3x` is now `px_6x`, holding the 6x pixel size.
`size_1x` and the anchors haven't changed. Everything is **6x of app size** except `panel_spill.png`, which stays
1x (it is smooth). Everything was rendered at 12x with 512 samples and the OptiX denoiser, filtered to 6x in linear
light, then exported by the round's own `export_app.py` and `export_thumb.py` (copies in `../../tree/reflect/tools/*_hq.py`).
The gem's girdle line and contact shadow are drawn at 12x and filtered, because PIL draws them unantialiased. The
pack is about 2.1 MB (it was 0.7 MB).

## Code changes the swap needs (`crates/qrs-app/src/player/transport_art.rs`)

1. **Sizes in the test `every_asset_loads_at_its_size`.** `GEM` goes to (576, 576), `KEY_DELTA` to (384, 384),
   `THUMB_DELTA + 2` to (240, 240) and `TRACK + 1` to (384, 144). The room bases are in `../hall/`: gem
   (576, 576) twice, stone (384, 384), thumb (240, 240).
2. **Filtering (recommended, this is half of the softness).** Today every texture is uploaded at full size with
   trilinear mipmaps and drawn at `px(canvas)`. At 2.2 ppp the 3x art sat at LOD 0.45, a 45 % blend with the 1.5x
   mip, which is visibly mushy. At 6x it would sit at LOD 1.45. Do what `qrs_ui::glass` does instead:
   - once per `pixels_per_point` (and per `units` scale), premultiply if needed;
   - resample each texture to its device size, `canvas * px(1) * ppp` (`image::imageops::resize` with
     `CatmullRom`, or `Lanczos3`);
   - upload with `TextureOptions::LINEAR` and no mips;
   - key the textures by (ppp, scale).

   Additive layers (alpha 0) resample the same way, since their RGB is already premultiplied. The strip tiles
   keep their 64-point period at any integer device width. Round the device width, and set the tile step to that
   width over ppp so the tiles don't drift. Keeping mipmaps also works, it's just softer. GPU memory at full size
   with mips is about 32 MB for this pack, against about 3 MB when resampled to 2.2 ppp.
3. **Nothing else assumes 3x.** The gem's disc cut works out `k = width / GEM_CANVAS` itself, and every quad's UVs
   are fractions. Only the doc comments say "drawn at 3x" (the module header, `Art`).
4. Rebuild with `tools\dev-build.ps1`, and check the transport shots (`10_player`, `15_paused`,
   `57_player_gem_down`) at `--size 3080x1804`.

---

The original pack's guide follows. Its blend maths and anchors are unchanged. Read "3x" there as 6x.

# Transport bar asset pack (#8, approved)

These are the shell Go stone keys, the variant F tall button thumb, the resin fill with its coat-only red, and the
engraved obsidian `glow_gilt` gem. All files are sRGB 8-bit RGBA PNG with **premultiplied alpha**, at **3x** app
size (scale by `pixels_per_point / 3`). The only exception is `panel_spill.png`, which is at 1x. `manifest.json`
gives every file's 1x size, its anchor (which pixel is the object's centre or the bar start), its blend mode and
weight, plus the geometry and constants. Total size is about 0.7 MB. `tools/verify_assets.py` rebuilds the bar from
this folder alone, following the manifest (`../app_assets_check.png`).

## Blend modes in egui (no custom shader)

- **normal**: a premultiplied texture drawn with vertex colour `Color32::from_white_alpha`/grey(w). w scales the
  colour and alpha together, which is how layers fade.
- **additive**: the texels have **alpha 0**. egui blends premultiplied (`ONE, ONE_MINUS_SRC_ALPHA`), so alpha 0
  adds the RGB. Build the `ColorImage` from `Color32::from_rgba_premultiplied(r, g, b, 0)` as stored. Don't load it
  through an unmultiplied path, which zeroes the RGB. The vertex colour is grey(w).
- **fill_light (c, a)**: drawn with the normal blend, `out = fb·(1 − w·a) + w·c`. It moves the amber's clear coat
  toward crimson without turning the body coral.

## Per frame

```
glow = lit + (1 − lit)·0.12      I(d) = 1 / (1 + (d/120)^2)      p = 0.8
panel_spill    additive at the gem centre, w = glow^p
track pieces   left | 64 px tile … | right (the slot ends at slot_end_x)
fill pieces    left | tile (phase 0 at the slot start) … | cut at each tick | lead at the thumb centre (2 px feathers)
fill_light_*   the same rects; per-column w(x) = clamp((glow·I(d(x)) / I(150))^p, 0, 1) as the quads' vertex colours
keys           key_base_<state> then key_delta (additive, w = glow^p); the forward key flips u; then the « » icon
gem            gem_glow (w = lit; 0 when pressed) + gem_glow_dark (w = 1 − lit), then gem_{lit,dark}_<sign>_<state>
               blended by lit
thumb          thumb_base_<state>, then thumb_delta_a and thumb_delta_b for the nodes either side of d
               (139 / 300 / 587), t = ln(d/a)/ln(b/a), weights (1−t)·(glow·I(d)/I(a))^p and t·(glow·I(d)/I(b))^p
```

Here d is the distance from the gem centre (127, 833) to the stone's centre. In ref/10_player.png at 1x the slot
starts at (253, 846.5), so the gem sits at (−126, −13.5) from the slot start.

## The switch

- **Pressing pause:** `lit` goes 1 → 0 over 0.35 s (smoothstep) while the PAUSE sign stays. At 0, swap to PLAY.
- **Pressing play:** swap to PAUSE first, then `lit` goes 0 → 1 over 0.30 s.

The gem sprite always blends dark with lit for one sign, never two signs at once. All reflections follow `glow`.

## Anchors at a glance (1x)

| Object | Canvas | Anchor | Notes |
|---|---|---|---|
| gem | 96 × 96 | centre (48, 48) | 64 px key with a 16 px margin; hover is 1 px up and pressed 1 px down, both baked in |
| keys | 64 × 64 | centre (32, 32) | 48 px stone; gem-to-key distance 69 px; the delta is baked with the gem to the right (the back key) |
| thumb | 40 × 40 | centre (20, 20) | Goes on the bar centre line. Face-on, the same camera as the stones |
| strip pieces | 24 tall | bar centre line at y = 12 | The slot is 13 px tall and centred on that line; the fill is 9.5 px |

The x anchors of the strip pieces are in the manifest. The left pieces have the slot start at 6 px; the right
track piece has the slot end at 60 px.

Hover is drawn 1 px up at ×1.05, pressed 1 px down at ×0.94 with a tighter shadow; both are baked into the
`_hover` and `_pressed` files.

The `« »` icons are drawn in code: ink (44, 46, 50), 2 px round-capped strokes, and a light lip (255, 255, 255, 120)
0.9 px lower. The exact geometry is under `constants.icon` in the manifest.

Source: `tiles/reflect/` (`tools/export_app.py` builds this folder; NOTES.md has the method and the proof).
