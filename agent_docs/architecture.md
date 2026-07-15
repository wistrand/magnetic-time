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
| `src/field.rs` | The `Face` (rotating `HandMagnets` or the `SegClock` seven-segment readout), magnet layouts (`LayoutSpec`), field elements, analytic B and grad(|B|^2), string parsers shared by CLI and web attributes |
| `src/sim.rs`   | `SimParams` tunables, overdamped particle stepper, spatial hash, chains, drag coupling  |
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
   RGBA buffer (capped by `Style::max_px`), uploaded as an egui texture.
   Headless mode runs the same loop without a window and writes the buffer to
   PNG.

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

Latest work (2026-07-15): the chain-length question resolved (chains are
regime-dependent, absorbed by dense bands, not bond-limited; measured via
`--dump-positions` and the experimental `--chain-cone` probe, see
[research-chain-banding.md](research-chain-banding.md) finding 10); the
spatial-hash cell fix so wide chain_range/repulsion ratios are honored, not
truncated at 4 cells; CLI input validation via the single-owner `bounds`
table (`SimParams::validate` errors, web/sliders clamp the same limits); and
the `Face` abstraction adding a digital seven-segment readout (`--face seg`)
alongside the hands, with a disc seconds marker orbiting the HH:MM face.

## Deferred / gated work

- GPU path (old phase 6): only if CPU limits particle count. First move is
  rasterization via eframe's wgpu `PaintCallback`, not a compute-shader sim;
  the sim is neighbor-bound at current presets (profiling finding in
  [gotchas.md](gotchas.md)), so GPU field math would buy little.
- f32 + struct-of-arrays hot path: parked until a low-power target (e.g.
  Raspberry Pi) exists to measure on. The analytic gradient already removed
  the f32 cancellation hazard.
- Stirring advection (hands dragging bulk fluid): superseded in practice by
  drag coupling; revisit only if fluid-memory wakes are wanted.
- Real threads on wasm (wasm-bindgen-rayon + COOP/COEP): not worth it at
  current counts; the component runs single-threaded by design.
