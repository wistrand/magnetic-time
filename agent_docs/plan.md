# Plan: magnetic clock

> Status: in progress. Phases 1-4 done; next is phase 5 (tuning).

## Goal

A native Rust + egui clock whose hands carry magnets, with a liquid layer of
magnetic particles dragged by the moving field: tight cluster on the hour
hand, comet trail behind the second hand, visible particle chains along field
lines.

## Current state

Empty repository, docs only. The design is settled in
[design-simulation.md](design-simulation.md) and
[design-rendering.md](design-rendering.md); this plan sequences the build.

## Approach

Overdamped particle sim driven by an analytic dipole field from the hands,
chains via cutoff-limited dipole-dipole pair forces on a spatial hash, CPU
pixel-buffer rendering. Reasoning and rejected alternatives are in the design
docs; do not re-derive them here.

Crates: `eframe`/`egui`, `rayon`, `chrono` (or `time`), a small deterministic
RNG (`fastrand` or similar), `image` or `png` for dumps. Ask the owner before
adding anything heavier.

## Testing methodology

No automated tests. Every phase is verified by running the headless dump
(built in phase 1) and reading the PNG, plus the owner running the app
interactively. Dumps are deterministic (fixed seed/time/duration), so
before/after PNG comparison is valid. The invariants in
[../CLAUDE.md](../CLAUDE.md) must hold after every phase.

## Phases

Ordered so each phase is independently verifiable before the next starts.

### Phase 1: scaffold and verification harness

- [x] Cargo project, eframe window, continuous repaint
- [x] Clock face and three hands from local time, smooth second hand
- [x] Single clock source (`src/clock.rs`) with speed multiplier, wired to a slider
- [x] Rasterized composite buffer (`src/render.rs`) and headless `--dump` CLI
- [x] `.gitignore` (target/, docs/debug/); owner will `git init` themselves

**Verify:** done. Dumps at 10:08:30 and +90 sim-seconds both read correctly.
Note: interactive mode displays the same rasterized buffer as a texture (no
vector face layer at all), so screen and dump are identical by construction.

### Phase 2: field model and field debug views

- [x] Data-driven dipole layouts per hand (`src/field.rs`, tip magnet each)
- [x] B and grad(|B|^2) evaluation summed over all dipoles
- [x] Heatmap, quiver, and dipole-marker debug views, `--view` flag in headless
- [x] Dev panel with view toggles

**Verify:** done. Field lobes sit on the hand tips at 10:08:30 and 02:40:15
and rotate with the hands; quiver arrows converge on the tips. Sim/world
coordinates are clock-face units (center origin, dial radius 1.0, y down),
defined in `src/hands.rs`; hand geometry is shared between render and field.

### Phase 3: particle layer

- [x] Particle state (`src/sim.rs`), seeded SplitMix64 init, dish boundary
- [x] Overdamped step: field force, drag/mobility, Brownian noise, fixed dt,
      speed cap on the magnetic term only
- [x] Spatial hash grid and soft-core repulsion
- [x] Buffer rasterization as additive soft dots, cleared every frame
- [x] Velocity-coloring and hash-occupancy debug views; sim sliders in panel

**Verify:** done. At 120 and 600 sim-seconds: finite-size clusters on hour and
minute tips, comet trail behind the second hand, and an emergent furrow ring
of particles along the second hand's sweep circle. Velocity view confirms
fast particles only near the tips. Headless perf: roughly 13 sim-seconds per
wall second at the default particle count, single-threaded; add rayon in
phase 4-5 if chains make this worse.

### Phase 4: chains

- [x] Induced per-particle moments (unit local-B direction with a saturation
      weight `chain_w` = |B|/b_sat capped at 1)
- [x] Cutoff dipole-dipole pair forces on the spatial hash (5x5 cells, per-
      particle neighbor cap, summed-speed cap; constants atop `src/sim.rs`)
- [x] Stroke rendering aligned with local field; chain-bond debug view
      (`--view chains`)
- [x] Alternating-polarity strip layout (done early: `LayoutSpec` in
      `src/field.rs`, `--magnets` flag, per-hand combos in the dev panel)

**Verify:** done. Bond view shows strings of particles along field lines;
normal view shows spike/fur filaments around clusters and in the second-hand
comet. Chains cost ~2.4x headless runtime vs phase 3 (~6 sim-seconds per wall
second at default count); still fine interactively.

### Phase 5: tuning and character

- [ ] Tune mobility vs hand speeds until hour=cluster, second=comet reads well
- [ ] Per-hand magnet layouts chosen for contrast
- [ ] Stirring advection, evaluate on/off
- [ ] Palette, saturation highlight, face styling
- [ ] Record chosen defaults in the tunables struct; note outcomes in
      [gotchas.md](gotchas.md)

**Verify:** owner judgment on the running app; dumps at 1x and high time scale
attached for reference.

### Phase 6 (optional): GPU path

- [ ] Only if CPU limits particle count: wgpu `PaintCallback` rendering,
      possibly compute-shader sim

**Verify:** identical look to CPU path at same seed; higher particle count at
interactive frame rate.

## Open questions

- Target particle count (start around 20k, tune by feel and profiling).
- See design-doc open questions (saturation/mobility values, alignment torque).

<!-- When all phases land: fold the design, decisions, and what each phase
     verified into architecture.md, delete this file, note it in CLAUDE.md. -->
