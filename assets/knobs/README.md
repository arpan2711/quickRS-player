# Knob pack: the app-chrome knobs as real objects, reflecting the shared room (dark themes only)

In quickRS (P34.8, `crates/qrs-ui/src/knob_art.rs`) the room uses the simple method (`manifest.<style>.radial`), so the
`*_nm.png`, `*_mtint.png` and `vintage_ptr_nm.png` maps are left in the research folder and not shipped here.

These are Blender renders of `qrs-ui/src/knobs.rs`'s four `KnobStyle`s: Amp gold, Chrome, Pedal black and Vintage
chicken-head. All the numbers are in `manifest.json`, one entry per style. The tools that made the pack are in
`../tools/`: `knob.py` (Blender), `bake.py` (passes to pack), `newknob.py` (a CPU model of the runtime recipe below)
and `sheet.py`. Daylight and the classic look stay as they are; use this pack only when `t().dark`.

## Geometry

- **Master.** A 64 app px knob in a 96 px canvas, rendered at 6x, so every sprite is 576 px square. The knob's radius
  is 192 master px. The camera is orthographic and looks straight down, the same face-on view as the gem and the
  stones. The key light comes from the upper left (screen azimuth 135°, elevation 42°).
- **Scale rule.** For a knob of `size` app px, draw every sprite at `1.5 × size` app px, centred on knobs.rs's
  `(cx, cy)`. That centre is also the centre of rotation.
- **Resampling.** Resample the masters on the CPU once per (style, size, ppp), the same way knobs.rs caches its
  textures in `Store::Body`. Downscale premultiplied with an area/box filter. The masters are big (576 px for a 34 px
  knob), so don't let the GPU minify them without mips.
- **Angle.** Unchanged: `deg = -135 + 270 u`, clockwise from 12 o'clock.
- **Tick ring and values.** Still drawn in code, exactly as today (`tick_ring`). A baked ring would lose the lit
  accent and its glow, which follow the theme and the value.

## Layers, in draw order

1. **Ticks.** Code, as today.
2. **Shadow.** `<style>_shadow_{normal,hover,pressed}.png`: white with alpha. Tint it with the theme's `cast` colour
   (vertex colour = `cast`, premultiplied by alpha). It replaces `make_cast`.
   - hover: the knob is lifted 0.07 R, so the shadow is longer and softer;
   - pressed: the knob's height is squashed to 0.78, so the shadow is shorter and tighter.
3. **Body.** `<style>_body.png`: straight-alpha sRGB, everything that does not turn. Optional brightness by state:
   hover ×1.06, pressed ×0.94 (multiply the vertex colour).
4. **Room.** An additive egui mesh on the hall probe, using the method in `assets/hall/README.md`. See below.
5. **Indicator.** It turns. See below.

## Why only the indicator turns (the rotation approach)

The skirt, its knurling and the spun cap are rotationally symmetric. Their look comes entirely from the fixed studio:
the bowtie highlight on spun metal and the lit flanks of the knurl always face the light. If they were spun, the
lighting would spin with them, which looks wrong. A knurl that moves under fixed light can't be seen at 34–64 px, so
the body is one static, pre-lit sprite.

The part that does turn, the painted line or the chicken-head pointer, also has angle-dependent light:
- the line's own sheen changes by up to 28 levels between 0° spun to −135° and a real render at −135° (the domed cap
  and the paint's coat catch the key light);
- the pointer's highlight and its cast shadow must stay put.

So each indicator is shipped as **pre-lit frames every 15°**, 19 frames from −135° to +135°. The app picks the nearest
frame, `k = round((deg + 135) / 15)`, and spins that quad by the residual `deg − angles[k]`. The residual is at most
7.5°, so any lighting error is under the 4–6 levels measured for a whole 15° step
(`rms_levels_frame_spun_15deg_vs_next` in the manifest).

The quad is an `egui::Mesh` with 4 vertices rotated about the centre and UVs 0..1, or the same rotation applied to UVs.

- **Line styles** (amp, chrome, pedal): `<style>_ind_<deg>.png`, a square of `square_app_px_at_master × size / 64`
  app px, centred.
- **Vintage:** `vintage_ptr_<deg>.png`, the full canvas. Each frame holds the pointer and its shadow on the base and on
  the panel.

The frames are minimal-alpha layers: `frame_render = layer + (1 − a) × (shadow + body)` in premultiplied sRGB (the
app's blend). Draw them with normal "over" blending.

## The room layer (reflections)

Use the same method as `assets/hall/README.md`: the additive blend, `hall::additive`, at `hall::LEVEL`. Build a polar
mesh per knob size: 64 segments, a feather ring at R + 0.5 px with m = 0, and rings at the `radial` rows' r × R.

For each vertex you need a normal n, a reflectance m and a base colour. There are two ways to get them:

- **Simple (recommended).** Use `manifest.<style>.radial`: rows r → (n_r, n_z, m_rgb, cov, base_srgb). Rebuild the
  normal as `n = (n_r cos φ, n_r sin φ, n_z)`, where φ is the vertex's angle in the screen frame (y up).
  `base_srgb` is the angle-mean of the body over the Midnight panel. For a better base, sample the resampled body
  sprite at the vertex.
- **Exact.** Sample `<style>_nm.png` (RGB = n·0.5+0.5 in the screen frame, A = m luminance × coverage) and
  `<style>_mtint.png` (RGB = m_rgb / m_lum / 2, so gold tints its reflection) at each vertex. This is the same scheme
  as `thumb_nm.png`.

Use m **per channel** (`m_rgb`) in `additive`: `gm_ch = G × m_ch × LEVEL`. Gold reflects gold.

| style | probe | gain G | why |
|---|---|---|---|
| amp | `room_probe_blur` texture (blurred) | 1.0 | a brushed gold cap: satin, m ≈ 0.8, so G is low |
| chrome | blurred | 1.0 | brushed cap, polished knurl |
| pedal | `room_probe` (sharp) | 2.6 | glossy black plastic, m ≈ 0.05 |
| vintage | sharp | 2.6 | glossy bakelite base |

- **States.** The same factors as the glass keys: hover ×1.18, pressed ×0.6.
- **The pointer.** `vintage_ptr_nm.png` is the pointer at 0° (RGB n, A m), for an optional room layer on it. Rotate
  the vertex positions and the normals' xy by deg. It isn't needed: the pointer is pale, so an additive layer barely
  shows on it.

## Hover and pressed

`draw_knob_unit` gets `hot` from `dial_interact` and drops it (`let _ = hot`). Controls, the guide and the board
call `knobs::knob` without any state. To use the states, pass a state into `knobs::knob`: hovered, or dragged
(`dial_drag` for this kind). They change three things:
- the shadow sprite;
- the body factor;
- the room gain.

## Files

Per style:
- `body`
- 3 `shadow`s
- `nm`
- `mtint`
- 19 indicator frames

The vintage style also has `ptr_nm`. `manifest.json` covers all four styles, plus a `common` block. Total size
2.7 MB: amp 0.49, chrome 0.52, pedal 0.37, vintage 1.28.
