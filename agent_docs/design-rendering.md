# Design: rendering and debug views

How the sim gets to the screen, and how agents verify it without eyes on the
running app.

## Layer stack

Everything is drawn into one CPU RGBA pixel buffer by the software rasterizer
in `src/render.rs` (`draw_clock`, SDF-based anti-aliased primitives), in this
order:

1. Clock face: dial, rim, ticks (no numerals; text rasterization is not worth
   a font dependency).
2. Hands, under the particle layer (particles float above the hands in the
   fiction).
3. Particle layer (from phase 3).

Interactive mode uploads the buffer via `TextureHandle::set` and draws it as a
single image; egui contributes only the window, the dev panel, and the
pointer-magnet feedback ring. Headless mode writes the same buffer to PNG.
This replaces the earlier plan of a vector egui face in interactive mode: one
render path means dumps are identical to the screen by construction, not by
discipline. The wasm web component renders through the identical path.

The interactive buffer size follows the window (physical pixels) capped by
`Style::max_px` (default in `render.rs`; 0 = uncapped); the texture upscales
linearly. Headless `--size` is exact and uncapped.

## Particle rasterization

- The buffer is fully cleared every frame. No decay, no trails, no phosphor;
  this is an owner decision and an invariant in [../CLAUDE.md](../CLAUDE.md).
- Each particle draws as a short anti-aliased stroke aligned with the local
  field direction (falls back to a dot where the field is weak). Strokes are
  what make chains read as chains; see
  [design-simulation.md](design-simulation.md). Stroke color and length scale
  with the smoothed magnetization weight `w_disp` (base-to-hot palette lerp),
  and a global stroke-length multiplier lives in `Style`.
- Particle blending adapts to the background (`Theme` in `src/render.rs`,
  derived from `Style::bg` luminance): additive glow on dark backgrounds,
  subtractive ink on light ones (subtracting the color's complement tints
  toward the palette color and darkens as it accumulates). Palettes carry a
  separate saturated color per mode; face colors lerp from the background
  toward white or black. Debug overlays stay dark-tuned.
- Never draw particles as per-particle egui shapes; the tessellator cannot
  handle tens of thousands of primitives per frame.

Upgrade path if CPU rasterization becomes the bottleneck: eframe's wgpu
backend supports `PaintCallback` for GPU point/stroke sprites. Do not start
there.

## Debug views

Toggleable overlays, each a checkbox in the dev panel and a name in the
`--view` flag (comma-separated):

- Field magnitude heatmap (|B| on a coarse grid, color-mapped).
- Force quiver: `grad(|B|^2)` arrows on a grid.
- Dipole markers: position and polarity of every hand magnet.
- Particle velocity coloring (speed as hue) instead of the normal look.
- Chain bonds: line segments between interacting neighbor pairs.
- Spatial hash occupancy grid.

All field tuning happens against the heatmap and quiver. Overlays are tuned
for dark backgrounds; they stay legible but not pretty on light ones.

## Headless dump (agent verification path)

The primary way an agent checks its work is to render a frame to PNG and Read
it:

```bash
cargo run --release -- --headless --time 10:08:30 --sim-seconds 60 \
    --dump docs/debug/out.png [--view field,quiver,dipoles,velocity,hash,chains]
```

All interactive flags apply (see the commands block in
[../CLAUDE.md](../CLAUDE.md) or `--help`). Behavior: initialize at the given
display time, run the sim for the given number of display seconds at fixed
dt, rasterize one frame (composited clock + requested views), write PNG,
exit. No window is opened.

- Shares the exact simulation and rasterization code with interactive mode
  (invariant in [../CLAUDE.md](../CLAUDE.md)).
- Deterministic: fixed seed + time + sim-seconds gives an identical PNG, so
  before/after comparison is valid (byte-exact for pure refactors).
- Interactive mode has a "dump frame" button writing the current frame to
  `docs/debug/`.
- PNG encoding via the `png` crate.

`docs/debug/` is disposable output and gitignored. The pointer magnet does
not exist headless; `--grad-check` verifies field math without rendering.

## Dev panel

An egui side panel (vertical scroll for small windows) with sliders for every
tunable, the time-speed multiplier, magnet layout/shape/strength combos per
hand, palette and background pickers, debug view toggles, particle count
(live), reset, and the dump button. Native shows it by default; the web
component hides it unless the `dev-panel` attribute is set.
