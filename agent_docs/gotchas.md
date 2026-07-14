# Gotchas and findings

Non-obvious traps and decision history. Everything below is anticipated from
planning (no code exists yet); confirm or correct entries as implementation
lands, and add what implementation teaches.

## Numerics (anticipated)

- Dipole fields diverge as 1/|r|^3 at the source. Clamp the evaluation
  distance to a minimum radius or forces explode for particles that reach a
  magnet.
- The attraction near field maxima is stiff. Explicit integration needs the
  clamped dt plus a per-step speed cap, or particles overshoot and oscillate
  across the magnet. If capping visibly distorts motion, switch to
  semi-implicit or substep near maxima.
- Brownian noise must come from a seeded deterministic RNG or headless dumps
  stop being reproducible.

## egui / eframe (anticipated)

- Never render particles as per-particle egui shapes; tessellation collapses
  around thousands of primitives. The pixel-buffer texture path exists for
  this reason.
- An idle egui app repaints only on input. Call `ctx.request_repaint()` (or
  `request_repaint_after`) every frame or the clock freezes when the mouse
  stops.
- Recreate the texture on window resize; `TextureHandle::set` with a
  different-sized image than the displayed rect gives scaling artifacts.

## Findings from phase 3

- The MIN_DIST field clamp makes |B| flat inside the clamp radius, so the
  numeric gradient is near zero there and particles collect in a shell at the
  clamp boundary: tip clusters render as small donuts. Harmless at current
  sizes (the hand covers the hole); revisit in tuning if it reads badly.
- Cluster cores sit at the speed cap in the velocity view: captured particles
  jitter in the attraction/repulsion equilibrium instead of resting. Purely
  visual at dot scale; consider a rest deadzone during tuning if it shimmers.
- The second hand plows a visible furrow of particles along its sweep circle
  over repeated laps. Emergent, looks good, keep it.

## Findings from phase 4

- At the default chain threshold (b_sat in `SimParams`), chaining is active
  over most of the dial, giving an all-over fur texture. Striking, but if
  tuning wants chains only near the hands, raise b_sat; it scales both the
  pair force and the stroke look.
- Live particle-count changes must rebuild the spatial hash on truncation or
  stale indices in the buckets can index out of bounds before the next step.
  `Sim::set_count` handles this; keep it that way.

## Findings from the rayon optimization

- The sim's three passes are rayon-parallel; ~5x wall speedup at default
  count. Noise had to move from a shared sequential RNG to stateless
  per-(particle, step) streams to stay deterministic under threading. This
  changed the noise sequence: dumps differ in fine detail from pre-rayon
  runs but are still fully reproducible.
- The |B|^2 gradient is forward-difference (2 extra field evals) instead of
  central (4); no visible difference at GRAD_EPS in `src/sim.rs`.
- When benchmarking, build first; `time cargo run` after an edit measures
  the compile, not the sim.

## Findings from the analytic gradient

- grad(|B|^2) is analytic (`FieldSources::b_and_grad_b2`: accumulate B and
  the Jacobian in one sweep, gradient = 2 J^T B), replacing forward
  differences. Verify with `--grad-check` after any field-element change; it
  compares against central differences. Expect a small mean error and O(1)
  outliers at r_min clamp kinks and very near magnets; those are the numeric
  reference's error (stencil straddling the kink, truncation on 1/r^7
  curvature), not the analytic value's.
- The sim is neighbor-bound at the owner presets, not field-bound: at the
  rings preset the chain/repulsion neighbor pass is ~3/4 of runtime, so the
  analytic gradient bought only a few percent there. Particle density in
  clumps drives cost; field-eval optimizations only pay in many-element or
  sparse configs. Measure before optimizing further.
- Removing the differencing error changed forces slightly; long runs diverge
  in fine detail from pre-change dumps (chaotic system), same character.
  One-time visual re-baseline, accepted 2026-07-14.

## Decision history

- Motion trails / phosphor decay: rejected by the owner. The buffer clears
  fully every frame. Do not reintroduce trails as a "cheap improvement".
- Chain textures: explicitly requested by the owner; simulated with real pair
  forces, not faked with oriented strokes alone. See
  [design-simulation.md](design-simulation.md).
- Fluid solver: rejected during planning as needless complexity for the
  desired look.
