# Plan: magnetic clock

> Status: in progress. Phase 1 done; next is phase 2.

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

- [ ] Data-driven dipole layouts per hand (start: tip magnet on each hand)
- [ ] B and grad(|B|^2) evaluation summed over all dipoles
- [ ] Heatmap, quiver, and dipole-marker debug views, `--view` flag in headless
- [ ] Dev panel with view toggles

**Verify:** dump `--view field` and `--view quiver` at two different times;
field lobes sit on the hand tips and rotate with the hands.

### Phase 3: particle layer

- [ ] Particle state, deterministic seeded init, circular dish boundary
- [ ] Overdamped step: field force, drag/mobility, Brownian noise, clamped dt,
      speed cap
- [ ] Spatial hash grid and soft-core repulsion
- [ ] Buffer rasterization as dots (strokes come with chains), additive blend,
      cleared every frame
- [ ] Velocity-coloring and hash-occupancy debug views

**Verify:** dump after `--sim-seconds 120` at time scale 1: particles
clustered at hand tips, no collapse to a point, empty dish elsewhere. Dump at
higher time scale: comet trail behind the second hand.

### Phase 4: chains

- [ ] Induced per-particle moments with saturation cap
- [ ] Cutoff dipole-dipole pair forces on the spatial hash
- [ ] Stroke rendering aligned with local field; chain-bond debug view
- [ ] Alternating-polarity strip layout as a second hand-magnet option

**Verify:** dump `--view chains` and the normal view: visible strings of
particles along field lines near magnets, not amorphous blobs; sim still
interactive-speed at target particle count.

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
