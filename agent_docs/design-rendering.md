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
single image; egui contributes only the window and the dev panel. Headless
mode writes the same buffer to PNG. This replaces the earlier plan of a vector
egui face in interactive mode: one render path means dumps are identical to
the screen by construction, not by discipline.

## Particle rasterization

- The buffer is fully cleared every frame. No decay, no trails, no phosphor;
  this is an owner decision and an invariant in [../CLAUDE.md](../CLAUDE.md).
- Each particle draws as a short anti-aliased stroke aligned with the local
  field direction (falls back to a dot where the field is weak). Strokes are
  what make chains read as chains; see
  [design-simulation.md](design-simulation.md).
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

Toggleable overlays, each with a keyboard shortcut and a checkbox in the dev
panel:

- Field magnitude heatmap (|B| on a coarse grid, color-mapped).
- Force quiver: `grad(|B|^2)` arrows on a grid.
- Dipole markers: position and polarity of every hand magnet.
- Particle velocity coloring (speed as hue) instead of the normal look.
- Chain bonds: line segments between interacting neighbor pairs.
- Spatial hash occupancy grid.

The field heatmap exists before any particles do (plan phase 2); all field
tuning happens against it.

## Headless dump (agent verification path)

The primary way an agent checks its work is to render a frame to PNG and Read
it. Planned CLI:

```bash
cargo run --release -- --headless --time 10:08:30 --sim-seconds 60 \
    --dump docs/debug/out.png [--view particles|field|quiver|chains] [--seed N]
```

Behavior: initialize at the given display time, run the sim for the given
number of display seconds at fixed dt, rasterize one frame (composited clock +
requested view), write PNG, exit. No window is opened.

- Shares the exact simulation and rasterization code with interactive mode
  (invariant in [../CLAUDE.md](../CLAUDE.md)); only the egui window and vector
  face layer differ, so the face is also rasterized into the dump buffer in
  headless mode.
- Deterministic: fixed seed + fixed time + fixed sim-seconds gives an
  identical PNG, so before/after comparisons are meaningful.
- Interactive mode gets a "dump frame" key writing the same PNG to
  `docs/debug/`.
- PNG encoding via the `image` crate (or `png` crate if `image` pulls too
  much; decide at implementation).

`docs/debug/` is disposable output and gitignored.

## Dev panel

An egui side panel (collapsible) with sliders for every tunable in the
tunables struct, plus time-scale multiplier, pause, single-step, particle
count, and the debug view toggles. Exists from the first sim phase; tuning is
the highest-risk part of the project and must be interactive.
