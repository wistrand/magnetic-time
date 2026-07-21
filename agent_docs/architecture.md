# Architecture

Module map and data flow. The build plan (plan.md) was promoted into this
file after all phases landed; design rationale lives in
[design-simulation.md](design-simulation.md) and
[design-rendering.md](design-rendering.md), decision history and traps in
[gotchas.md](gotchas.md).

## Modules

| File           | Role                                                                                   |
|----------------|----------------------------------------------------------------------------------------|
| `src/main.rs`  | Native CLI (flags, headless mode, --grad-check) and both entry points (native, wasm)   |
| `src/clock.rs` | The single time source: display seconds since midnight, speed multiplier, HH:MM:SS I/O |
| `src/hands.rs` | Hand lengths and angles; defines clock-face units (center origin, dial radius 1, y down) |
| `src/field.rs` | Faces: `FaceConfigs` (the Copy config every carrier holds) builds a `Face` (rotating `HandMagnets`, the `SegClock` seven-segment readout, or the `TideClock` arcs); magnet layouts (`LayoutSpec`), field elements, analytic B and grad(|B|^2), string parsers shared by CLI and web attributes |
| `src/sim.rs`   | `SimParams` tunables, overdamped particle stepper, spatial hash, chains, drag coupling  |
| `src/preset.rs`| JSON presets: `to_json` / `apply_json` over `(FaceConfigs, SimParams, Style, speed)`, hand-rolled flat-JSON reader/writer (no serde), lenient apply |
| `src/render.rs`| Software rasterizer, `Style`/`Palette`/`Theme`, debug overlays, PNG output              |
| `src/app.rs`   | eframe app: pending-config channel, pointer magnet, dev panel, fixed-dt catch-up loop   |
| `src/web.rs`   | wasm-only `WebHandle` (start/destroy + attribute setters) behind the web component      |
| `docs/app/magnetic-clock.js` | The `<magnetic-clock>` custom element wrapping `WebHandle`               |

## Data flow per frame (interactive)

1. `ClockApp::update` drains the pending config (web component pushes), reads
   the pointer, and steps the sim in fixed dt (`SimParams::dt`, default 1/30
   display-second; quantitative pattern work needs 1/120, see
   [gotchas.md](gotchas.md)) toward the current display time under a 12 ms
   wall budget; excess display time is dropped (hands stay truthful,
   particles skip).
2. Each sim step: `FieldSources::at_time` expands the current `Face` into
   world elements (hands rotated by time, or the seg readout's switched bars),
   plus the pointer magnet; pass 1 samples B and grad(|B|^2) analytically per
   particle, pass 2 sums neighbor forces on the spatial hash, optional pass
   2.5 smooths velocities (drag coupling), pass 3 integrates.
3. `draw_clock` rasterizes face, magnets, particles, and overlays into one
   RGBA buffer (capped by `Style::max_px`), uploaded as an egui texture. The
   particle pass is rayon-parallel over horizontal bands (byte-exact vs a
   serial pass). Headless mode runs the same loop without a window and writes
   the buffer to PNG.

## Verification methodology

No automated tests (owner rule). Changes are verified by headless PNG dumps
read by the agent plus the owner running the app:

```bash
cargo run --release -- --headless --time 13:37:35 --sim-seconds 240 --dump out.png
magnetic-time --grad-check     # after any field-element change
magnetic-time --headless ... --dump-positions out.csv   # positions + local
                               # field for measurement scripts; image-based
                               # estimators fuse overlapping dots (finding 10)
```

Dumps are deterministic (fixed seed/time/duration, order-independent passes,
index-keyed noise streams), so before/after comparison is valid; byte-compare
for refactors that must not change behavior, visual compare otherwise. Keep
`cargo check --target wasm32-unknown-unknown` green. The invariants in
[../CLAUDE.md](../CLAUDE.md) must hold after every change. Quantitative
image analysis for the research experiments lives in `scripts/*.py`
(numpy+PIL); see
[research-chain-banding.md](research-chain-banding.md) for what each
measures and its calibration caveats.

## Build-plan history

Phases 1-5 of the original plan are built and verified: scaffold + headless
harness, analytic field + debug views, particle layer, chains, tuning (the
owner presets baked into the `Default` impls). Shipped beyond the plan:
per-hand magnet strengths and shapes (disc, hand-relative bars), chain
geometry parameters and compression, XSPH drag coupling, analytic gradient
with `--grad-check`, palettes and background theming with adaptive ink
blending, the wasm build and `<magnetic-clock>` web component, the pointer
magnet, and the resolution cap. After that, the band-physics research
program ([research-chain-banding.md](research-chain-banding.md)): the
analysis scripts in `scripts/`, the exposure of every remaining sim
constant (dt, field_clamp, chain caps, repulsion radius), the nearest-N
neighbor selection, the fluid_scale band-size dial, and the public writeup
docs/banding.html.

Latest work (2026-07-21): the chain-length question resolved (chains are
regime-dependent, absorbed by dense bands, not bond-limited; measured via
`--dump-positions` and the experimental `--chain-cone` probe, see
[research-chain-banding.md](research-chain-banding.md) finding 10); the
spatial-hash cell fix so wide chain_range/repulsion ratios are honored, not
truncated at 4 cells; CLI input validation via the single-owner `bounds`
table (`SimParams::validate` errors, web/sliders clamp the same limits); and
the `Face` abstraction adding a digital seven-segment readout (`--face seg`,
with a disc seconds marker orbiting the HH:MM face) and the tide arcs
(`--face tide`) alongside the hands, all carried by one grouped `FaceConfigs`
so a new face is a field.rs-local change; and JSON presets (`src/preset.rs`,
`--preset` / `--save-preset`, dev-panel save/load, web `get_preset` /
`set_preset`) with a serde-free flat-JSON reader; a `--no-dev-panel` flag and
an optional FPS overlay (`--fps`); a `Makefile` for the common tasks; and a
parallel (banded) particle rasterizer with a tighter per-row capsule scan
that removed the last serial hot path (byte-exact, ~2.5-3.5x on the render
pass); an f32 hybrid for the particle state (halving the memory the
neighbor pass gathers, for the bandwidth-bound Pi target; field pass stays
f64); a heatmap render mode (`--heatmap N`) whose cost is independent of
clustering and stroke length (the cheap render path for the Pi and the answer
to the banding FPS drop); spatial (Morton) reordering of particles for
gather locality (flat on desktop, gated on a Pi measurement; see gotchas.md);
a pointer magnet that can repel as well as attract (`--pointer-repel`, a
separate outward push since a charge's field-magnitude force is
sign-independent); and a palette redo to a two-color `start -> end` ramp
interpolated in OKLab (`Palette { start, end }`, baked to a 256-entry LUT per
frame; `--palette NAME|startHex-endHex`, background separate), replacing the
named base/hot enum.

## Deferred / gated work

- GPU path (old phase 6): only if CPU limits particle count. First move is
  rasterization via eframe's wgpu `PaintCallback`, not a compute-shader sim;
  the sim is neighbor-bound at current presets (profiling finding in
  [gotchas.md](gotchas.md)), so GPU field math would buy little.
- f32 particle state: BUILT (hybrid). `Vec2f`/f32 per-particle arrays and
  hot loops (neighbor/drag/integrate); the field pass stays f64 (`Vec2`,
  queried per particle) for near-source accuracy. Struct-of-arrays was
  REJECTED: the neighbor pass gathers scattered neighbors, so f32-AoS (one
  8-byte read per gather) beats splitting x/y into two arrays (two cache
  lines). See gotchas.md.
- Stirring advection (hands dragging bulk fluid): superseded in practice by
  drag coupling; revisit only if fluid-memory wakes are wanted.
- Real threads on wasm (wasm-bindgen-rayon + COOP/COEP): not worth it at
  current counts; the component runs single-threaded by design.
- Magnetophoretic display simulator (adjacent application, not built). This
  engine already models the exact physics of magnetophoretic e-paper (Magna
  Doodle, magnetic rewritable signage): ~10 um magnetic particles in a
  viscous medium moved by a magnetic head, overdamped. The pointer magnet is
  the stylus. One ingredient is missing to make it a driver-prototyping tool:
  BISTABILITY, i.e. particles holding their state after the field leaves (a
  yield-stress / stiction threshold in the velocity, below which a particle
  does not move). This is a PHYSICS addition, not a rendering trail, so it
  does NOT conflict with the no-trails invariant (that invariant is about
  buffer decay; particles are already stateful frame to frame). If built,
  gate the hold on a force threshold, not on time. Relevance is direct for
  magnetophoretic displays; for mainstream electrophoretic (E Ink) it is only
  a framework/intuition match (ghosting <-> the ghost-decay experiment,
  flocculation <-> chaining, color-pigment sorting <-> a two-species
  extension, pixel fringing <-> gradient banding). The project's headline
  results (tidal delta*, fifth-root insensitivity) do NOT transfer to E Ink:
  those are gradient-driven (grad|B|^2) induced-dipole physics, whereas E Ink
  is uniform-field transport of permanently charged particles. Refs:
  US patent 10,444,553 (magnetophoretic display driving scheme); E Ink ACeP
  color and clearing-waveform / ghosting literature (MDPI Micromachines).
