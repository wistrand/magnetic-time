# Design: rendering and debug views

How the sim gets to the screen, and how agents verify it without eyes on the
running app.

## Layer stack

Everything is drawn into one CPU RGBA pixel buffer by the software rasterizer
in `src/render.rs` (`draw_clock`, SDF-based anti-aliased primitives), in this
order:

1. Clock face statics: background fill, dial, rim, and (hands face only) the
   60 minute ticks. No numerals; text rasterization is not worth a font
   dependency. The digital seven-segment face skips the ticks, which would
   read oddly behind it. The dial and rim follow the dish shape
   (`SimParams::dish`): the plain circle uses the original disc/ring calls,
   shaped dishes fill/outline the SDF, which draws hole rims for free.
   Style options: `outside_bg` recolors only the area outside the dial,
   `transparent_bg` makes it alpha-0 (with a transparent window, only the
   circular dial is visible; headless PNGs get transparent corners),
   `face_color` fades the rim/tick colors from bg toward one accent color,
   `show_face` off skips the statics entirely. Interactive mode caches this
   whole layer (`FaceLayer`): re-rasterized only when its key changes (size,
   colors, transforms, dish, face kind), otherwise the frame starts with a
   memcpy. Measured 10.4 -> 3.0 ms/frame at 700 px; byte-identical because
   the template is produced by the same rasterization code it replaces.
   Headless passes no cache and rasterizes directly. The buffer is still
   fully overwritten every frame, so the no-trails invariant holds.
   Any new Style field that affects the statics must join `FaceLayerKey` or
   stale frames ship.
2. The face magnets, under the particle layer (they float below the particles
   in the fiction), and only when `Style::show_hands` is set. Hands draw as
   capsules from the time-derived angles; the seg and tide faces share one
   branch that draws each bar (and seg's colon/orbit discs) from
   `sources.markers` (world-space, so no face geometry is duplicated in the
   renderer, and a new marker-emitting face draws for free). All default off:
   the particles carry the reading.
3. Particle layer.

All drawing goes through `Map` (world to pixel), which also applies the view
transforms: `Style::rotate` (degrees, clockwise) and `Style::flip_x` /
`flip_y` (axis mirrors, applied before rotation). Pure presentation: sim,
field, and `--dump-positions` stay in world space; the pointer mapping
applies the inverse, so touches land where pressed and the 12 o'clock
hotspot follows the transformed tick. Verified pixel-exact against
image-space rotation/mirroring of the untransformed render.
Never draw world geometry with explicit trig around the center; route
endpoints through `Map::px` or transforms will miss it (the hands were
converted for exactly this reason).

Interactive mode uploads the buffer via `TextureHandle::set` and draws it as a
single image; egui contributes only the window, the dev panel, the FPS
overlay, and the pointer-magnet feedback rings (one per touch). Headless
mode writes the same buffer to PNG.
This replaces the earlier plan of a vector egui face in interactive mode: one
render path means dumps are identical to the screen by construction, not by
discipline. The wasm web component renders through the identical path.

The interactive buffer size follows the window (physical pixels) capped by
`Style::max_px` (default in `render.rs`; 0 = uncapped); the texture upscales
linearly. `Style::pad` reserves a margin around the dial as a fraction of
the window's short side. Headless `--size` is exact and uncapped. Window
options are interactive-only: `--fullscreen` (borderless, Esc quits),
`--window-size WxH`, and `--kiosk` (fullscreen + panel hidden + overlay
panel + centered fps + outside_bg defaulting to black).

## Particle rasterization

- The buffer is fully cleared every frame. No decay, no trails, no phosphor;
  this is an owner decision and an invariant in [../CLAUDE.md](../CLAUDE.md).
- Each particle draws as a short anti-aliased stroke aligned with the local
  field direction (falls back to a dot where the field is weak). Strokes are
  what make chains read as chains; see
  [design-simulation.md](design-simulation.md). Stroke color and length scale
  with the smoothed magnetization weight `w_disp` (start-to-end palette lerp),
  and a global stroke-length multiplier lives in `Style`.
- Particle blending has two modes (`Theme::ink_add` in `src/render.rs`):
  additive glow (dense marks climb toward white) and subtractive ink
  (subtracting the color's complement tints toward the palette color and
  darkens as it accumulates). The mode is chosen by comparing the palette's
  `end` (the dense-crest color) against `Style::bg` luminance: additive when
  the ink is brighter than the bg, subtractive when darker, so the densest
  marks always contrast with the canvas. Keying it to bg luminance ALONE (the
  old rule) breaks once the palette is a free start/end range: a bright
  preset on a light bg would blend additively into invisibility. Face colors
  are separate and still lerp from the background toward white or black by bg
  luminance (`Theme::dark`). Debug overlays stay dark-tuned.
- A palette is `Palette { start, end }` (`src/render.rs`): two sRGB colors,
  the whole ramp interpolated in OKLab so the gradient is perceptually even.
  Particle color runs from `start` (low velocity, dim) to `end` (band crests,
  max). `--palette` takes either a preset name (`ice|ember|emerald|violet|
  mono`, in `Palette::PRESETS`) or a custom `startHex-endHex` pair; the panel
  exposes both endpoints as color pickers plus preset buttons. `Palette::lut`
  bakes the ramp to a 256-entry sRGB table once per frame, so per-particle
  color is a table lookup, not an OKLab conversion. Background (`Style::bg`) is
  separate and drives the theme, not the ramp. On dark backgrounds pick an
  `end` short of pure white or additive accumulation still blows dense cores
  out; a saturated `end` (a channel near 0) instead saturates to that hue.
- Never draw particles as per-particle egui shapes; the tessellator cannot
  handle tens of thousands of primitives per frame.
- The particle pass is parallel: `draw_particles` splits the buffer into
  horizontal bands (`par_chunks_mut`, ~3x cores) and each band rasterizes all
  particles clipped to its rows. Each pixel belongs to exactly one band and
  particles are walked in index order per band, so the read-modify-write
  blends never race and the per-pixel blend order matches a serial pass;
  output is byte-identical (verified against the pre-parallel baseline). rayon
  falls back to one sequential pass on wasm. Cost was the render bottleneck
  because it was serial while the sim used all cores.
- Stroke cost scales with the stroke's pixel area, so long strokes are
  expensive. The band rasterizer (`raster_capsule`) iterates only the per-row
  x-span the stroke can cover (the infinite-line strip of half-width `hw`
  sliced at each row), not the full AABB, skipping the corners a diagonal
  stroke never touches. This is a strict superset of covered pixels (distance
  to the segment >= distance to the line), so it stays byte-identical. The
  `Framebuffer::capsule_ink` method keeps the old full-AABB scan for the
  chains debug view.

- Heatmap render mode (`Style::heatmap_res > 0`, `--heatmap N`, `heatmap`
  attribute, panel slider): `draw_heatmap` replaces `draw_particles`, binning
  particles into an NxN density grid over the dish and colouring each pixel by
  its cell's count (log-scaled, self-normalised, start->end ramp via the same
  `Palette::lut`, parallel
  bands). Cost is O(particles) to count + O(pixels) to colour, INDEPENDENT of
  clustering and stroke length: a dense cell is one increment where strokes
  would draw many overlapping long strokes (measured ~12x cheaper than strokes
  at 2600 px on a banded state, and roughly constant). This is the answer to
  the clustering/banding FPS drop, and the cheap render path for the Pi.
  `heatmap_res` is the grid resolution (blocky when small); 0 keeps strokes.

Further upgrade path if CPU rasterization is still the bottleneck: eframe's
wgpu backend supports `PaintCallback` for GPU point/stroke sprites, which
would make stroke length nearly free but breaks the shared-rasterizer
invariant (GPU rounding differs from the CPU headless path). Do not start
there.

## Debug views

Toggleable overlays, each a checkbox in the dev panel and a name in the
`--view` flag (comma-separated):

- Field magnitude heatmap (per-pixel |B|, log-scaled, self-normalized per
  frame).
- Force quiver: `grad(|B|^2)` arrows on a grid.
- Dipole markers: position and polarity of every hand magnet.
- Particle velocity coloring (speed as hue) instead of the normal look.
- Chain bonds: line segments between interacting neighbor pairs.
- Spatial hash occupancy grid.
- Camera walls: the camera obstacle field's blocked regions shaded red with
  a bright boundary line (interactive with --camera only; headless has no
  wall grid, so the view draws nothing there).

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

An egui panel, docked or floating (see below; vertical scroll for small
windows). Ordered most-used
first: speed, the face selector (hands / seg / tide, with each face's own
controls), a collapsible `magnets` section for the per-hand layout combos
(hands mode), then particle count (live) and reset, the common look (show
hands/magnets, stroke length, palette, background), then a short "physics"
block of the most-touched knobs (mobility, max speed, noise, chain strength,
repulsion, fluid scale). The rarely used tunables live in collapsing sections
(`chain detail`, `field & fluid`, `pointer / touch`, `render`), and the debug
view toggles in their own collapsing section, so the panel is short by
default. The per-hand magnet loop is factored into `ClockApp::magnet_controls`
so the collapsible wrapper stays a few lines. Slider ranges come from the
shared `bounds` table in `src/sim.rs`, not inline literals. A dish text row
(CLI grammar + apply button) edits the container shape live. A native-only
preset row (path field + save/load) serializes the whole config to JSON via
`src/preset.rs`; the CLI has `--preset` / `--save-preset` and the web handle
`get_preset` / `set_preset` (exposed as `savePreset()` / `loadPreset()` on the
`<magnetic-clock>` element). Below it, native-only: an autosave checkbox
(persists config changes to `preset::autosave_path()`, throttled and
change-gated; file presence means enabled and the next interactive start
loads it as the base config, explicit flags overriding; unchecking deletes
the file; `--no-autosave` ignores it for a run; headless / `--grad-check` /
`--save-preset` runs never load it, for reproducibility), and a dump frame /
exit button row.

The panel has two homes (`Style::panel_overlay`, `--dev-overlay`, "overlay"
checkbox): the docked right `SidePanel` (takes layout space, shrinks the
clock) or a floating egui window over the dial (the clock keeps full size;
draggable by its title bar, close button = the hotspot toggle, default
position right edge vertically centered, session-only placement memory).
Native shows the panel by default (`--no-dev-panel` starts hidden); the web
component hides it unless the `dev-panel` attribute is set. Tapping the 12
o'clock tick toggles it anywhere (native and web); the pointer magnet is
suppressed inside that hotspot so the tap does not stir the particles.
Raw pointer input reaches the sim only when egui does not own it; see the
input-ownership entry in [gotchas.md](gotchas.md).

An optional FPS overlay (`Style::show_fps`, `--fps`, `fps` attribute, panel
checkbox) draws the smoothed frame rate as an egui label, top-left by
default or top-center with `Style::fps_center` (`--fps-center`, kiosk
default), independent of the panel.
