# Hall pack, HQ (6x)

This is a drop-in for `quickRS-rust/assets/hall/`. It keeps the same file names, and `manifest.json` keeps the same
structure, with `"scale": 6` added at the top. The sprites and maps are **6x of app size** (they were 3x before),
except `glass_normal.png`, the legacy radius-7 look that the app only uses in tests, which stays 3x. Everything
was rendered at 12x with 512 samples and the OptiX denoiser, then filtered to 6x in linear light. See
`../../NOTES.md` for how. The whole pack is about 3.1 MB (it was 0.6 MB).

## What changed

| file | before | now |
|---|---|---|
| `glass_r16_{normal,hover,pressed}.png` | 768x210 | 1536x420, with faint micro-scratches and waviness in the top face, and 32 segments a corner |
| `glass_disc_*.png` | 216x216 | 432x432 |
| `gem_lit_pause.png`, `gem_dark_play.png` | 288 | 576 (the girdle line and shadow are drawn at 12x and filtered) |
| `stone_base.png` / `thumb_base.png` | 192 / 120 | 384 / 240 |
| `thumb_nm.png` | 120 | 240 |
| `room_probe.jpg` | 512, q92, 4:2:0 | 1024, q96, 4:4:4 (334 KB). 1024 is the 2048 equirect master's real detail, so 2048 would only interpolate |
| `room_probe_blur.jpg` | 256 q92 | 256 q95 4:4:4 |
| manifest tables | | re-measured on the new bakes: `glass_r16`/`glass_disc` rings and face_m, `gem.facets`, `go_stone.radial`, `resin.profile` |
| `ref/ref_r16_*`, `ref/ref_disc_*` | | redrawn at the new tables. The `ref_key_*` R7 references are unchanged |

## Code changes the swap needs

1. **`crates/qrs-ui/src/glass.rs`**
   - `SPRITE_K` (3.0) becomes per look: add `k: f32` to `Look` (`RECT` and `DISC` 6.0, `R7` 3.0).
   - Use `look.k` in `sprite_under` (pass the look, or k, in from `geom`) and in the test
     `every_sprite_loads_at_its_size`.
   - Replace the `RECT` and `DISC` `rings` and `face_m` with the manifest's (the test `the_tables_are_the_manifests`
     checks them):
     ```
     RECT face_m 0.08165, rings:
        (-0.5, 0.0, 0.0),
        (0.0, 39.93, 0.03415),
        (0.2, 72.65, 0.059),
        (0.45, 64.34, 0.04557),
        (0.8, 55.23, 0.04868),
        (1.25, 45.97, 0.09347),
        (1.8, 36.38, 0.08587),
        (2.5, 25.72, 0.08261),
        (3.4, 13.23, 0.0815),
        (4.4, 2.53, 0.08156),
        (5.6, 0.0, 0.08172)
     DISC face_m 0.08139, rings:
        (-0.5, -0.0, 0.0),
        (0.0, 47.99, 0.04142),
        (0.2, 69.79, 0.05468),
        (0.45, 59.89, 0.04278),
        (0.8, 49.55, 0.08596),
        (1.25, 38.94, 0.08702),
        (1.8, 27.82, 0.08302),
        (2.5, 15.11, 0.08189),
        (3.4, 2.38, 0.08178),
        (4.4, -0.0, 0.08162),
        (5.6, -0.0, 0.08159)
     ```
   - `textures()` needs no change. It already resamples the sprite to the device scale once per ppp, now always
     *down* (the disc's `ppp * 64/48` upscale was what softened round keys from 2.25 ppp). Optionally use
     `FilterType::CatmullRom` (or Lanczos3) instead of `Triangle` for a slightly crisper arris at 1x and 2x.
2. **`crates/qrs-ui/src/hall.rs`**
   - The test `the_rooms_textures_load_at_their_sizes` changes `512` to `1024`.
   - **Give the probe and the two mix textures mipmaps**:
     `TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..TextureOptions::LINEAR }`. Without them,
     a key's arris minifies the probe about 24x (now about 48x), and the roof's bright lines alias into the crawling
     dashes the owner saw. The flat faces magnify, so mips cost them nothing. The CPU copy (the blurred probe)
     doesn't change.
3. **`crates/qrs-app/src/player/transport_room.rs`**
   - `NM_K` changes from 3.0 to 6.0.
   - The test `every_asset_loads_at_its_size` changes `(120, 120)` to `(240, 240)` for `thumb_nm.png`.
   - The gem, stone and resin tables are read from the manifest at load, so nothing else changes.
4. **`crates/qrs-app/src/player/transport_art.rs`** (it uses this pack's four bases)
   - The test's room sizes change: gem 576, stone 384, thumb 240.
   - The gem's disc cut works out k from the image width already.
   - See `../transport/README.md` for the texture filtering.
5. **Docs and comments** that say "3x": `glass.rs` (header, `SPRITE_K`, `sprite_under`), `hall.rs` header, and
   `transport_room.rs` (`NM_K`).

GPU memory for this pack, as RGBA8: about 19 MB at full size. `glass.rs` keeps only its per-ppp resampled copies on
the GPU, so the plates cost what they always did on screen.

---

The original pack's guide follows. The method is unchanged, and the sizes in it are the 3x ones.

# Room app pack: one hall reflected by every glossy thing, egui, no shaders

All numbers are in `manifest.json`. The pack is made by `../tools/export_app.py`, on the CPU only. The method is
explained in `../NOTES.md`, and the prototype it mirrors is `../tools/rt.py`.

## Textures

| file | size | upload | what it is |
|---|---|---|---|
| `room_probe.jpg` | 512² | linear filter, clamp, no mips, bytes as-is (not sRGB-decoded) | the room as a sphere map; bytes = clamp(2.5 × linear radiance) |
| `room_probe_blur.jpg` | 256² | same | blurred copy, used for the thumb |

- Keep the decoded `room_probe_blur.jpg` on the CPU. It gives the local radiance per vertex: L = bilinear(bytes)/255/2.5.
- For the stones and the thumb, build one more texture at load: t = srgb(min(1, 1.7·bytes/255/2.5)).

## Per frame, per reflective surface: one `egui::Mesh`

For each vertex (sprite-space position p, normal n in the screen frame (x right, y up, z out), reflectance m, base
colour under it):

```
P  = sprite_top_left + p                     (app px, y down)
v  = normalize(P.x - W/2, H/2 - P.y, -D)     D = window width in app px (eye 1 window-width away)
R  = v - 2 (v.n) n
m_ = 2 sqrt(2 (1 + R.z));  uv = (0.5 + R.x/m_, 0.5 - R.y/m_)
```

There are two blend modes.

**Additive** (glass keys, gem, resin). Colour = grey per channel, alpha 0:

```
x = max(G m L, 1e-5)
c = clamp(G m (srgb(lin(base)+x) - base) / x / 2.5, 0, 1)
```

**Mix** (Go stones, thumb). Premultiplied (a, a, a, a), with a = min(0.5, 1.85 m), on the sRGB mix texture:

```
out = fb (1 - a) + a t
```

Gains:

| surface | gain |
|---|---|
| glass key | G = 2.6 (hover ×1.18, pressed ×0.6) |
| gem | G = 6 (table facet m × 0.08 so the sign stays clean) |
| resin | G = 2.5 |

Draw order: base sprite, label or icon ink, then the room mesh. For the stones the order is base, room, the gem's
crimson delta, then the icon.

## Surfaces

- **Glass key, A2 Crown.**
  - Sprites: `glass_{normal,hover,pressed}.png`, 3x, a 256×70 app px canvas, 9-slice insets 22 px all round. They
    have no label, are straight-alpha over the panel colour (23,38,52), and their studio reflections are removed.
  - The main action uses the same plate with amber ink.
  - Label recipe: the manifest's `glass_key.label` (dark pass up-left, light pass down-right, the ink, a 5 % ghost
    offset by the plate's thickness).
  - Reflection mesh: rounded-rect rings at the insets in `glass_key.reflection_mesh.rings`. Each ring has a tilt and
    an m; each vertex's normal is its outward direction tilted by the ring's tilt. A feather ring sits at −0.5 px with
    m = 0, then face rings (n = +z, m = face_m) down to the mid-line.
- **Gem.** `gem.facets`: one row per make_gem facet, in its order (for j: kite, upper girdle a, upper girdle b, star;
  then the table), each with n, m and base. Use one fan per facet. Sprites: `gem_lit_pause.png`, `gem_dark_play.png`
  (3x, 96 px canvas, studio cards at 35 %).
- **Go stone.** `go_stone.radial`: rows of r → (n_r, n_z, m, base), for a polar grid with 64 segments. The forward key
  mirrors x and n.x. Sprite: `stone_base.png`.
- **Thumb (face-on button).** `thumb_nm.png` is a 3x normal + reflectance map (RGB = n·0.5+0.5, A = m). Sample it at
  the vertices of a polar grid (56 × 19, out to 13.2 px). Sprite: `thumb_base.png`.
- **Resin fill.** `resin.profile`: rows y → (n, m, base) across the fill, with columns every 8 px. Vertex m =
  profile m × the fill's coverage (0 in the beat cuts and past the lead end).

## Checking your render

`ref_key_at720_120_2x.png` and `ref_key_at300_780_2x.png` show the blank key at 2x (1440×900 window, gain 2.6):
`glass_normal.png` over the panel colour plus the room layer. `*_layer.png` is the layer alone, ×4.
`ref_key_verts.json` lists every vertex (x, y, n, m, base, u, v, Color32) and the triangles, so you can diff numbers
before you diff pixels. Rasterise at pixel centres with bilinear, clamped sampling.

This table-driven mesh is within 2.2 levels RMS of the Blender-bake-sampled mesh that was proven against Blender.

## Caveats

- `glass_hover.png` and `glass_pressed.png` were made on the CPU (round 2's renders minus their studio share), not
  re-rendered. A faint residue of the old window card, 1–2 levels, may show up left of centre.
- The gem has only its lit/pause and dark/play states. Hover and pressed come from lift/brightness in code.

## Round 3: rounder plate and a round disc (`glass_r16_*`, `glass_disc_*`)

- **`glass_r16_{normal,hover,pressed}.png`.** The same A2 Crown glass with a 16 px corner radius and a softer 4.4 px
  arris, 256×70 canvas at 3x. 9-slice insets are 30 px all round (`manifest.glass_r16`).
- **`glass_disc_{normal,hover,pressed}.png`.** A round 48 px plate (72 px canvas) at 3x. Scale it to 32–64 px, with
  its mesh scaled the same way.
- **Shadow.** Only a soft drop shadow (offset (1.0, 1.6) px, 22 %). There is no contact line or dark rim ring. The
  sprites are encoded against the render's own rubber, so no dark halo is left round the plate.
- **All three states are real renders.**
- **Ring tables** are in `manifest.glass_r16.reflection_mesh` and `manifest.glass_disc.reflection_mesh`. The disc mesh
  is 64 segments, plus face rings at 8, 14 and 19 px, plus a centre fan.
- **References.** `ref_r16_at*_2x.png` and `ref_disc_at*_2x.png` use gain 2.6. The app tones the room down to
  2.6 × 0.3.
