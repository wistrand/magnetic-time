# Gotchas and findings

Non-obvious traps and decision history, in rough chronological order. Add
what implementation teaches; correct entries that turn out wrong.

## Numerics (confirmed in practice)

- Dipole fields diverge as 1/|r|^3 at the source. Evaluation distance is
  clamped (MIN_DIST in `src/field.rs`, or the disc radius); without it forces
  explode for particles that reach a magnet. Side effect: the field plateaus
  inside the clamp, so tip clusters form shells (see phase-3 findings).
- The attraction near field maxima is stiff. The fixed dt plus per-term speed
  caps hold it stable; cluster cores jitter at the cap instead of resting
  (visually invisible at dot scale).
- All randomness comes from seeded deterministic RNG streams keyed by
  (particle, step) or headless dumps stop being reproducible.

## egui / eframe (confirmed in practice)

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

## Findings from the wasm port

- The browser build is single-threaded: rayon compiles for wasm and falls
  back to sequential execution. Particle count is reduced in the wasm entry
  point (`main.rs`) to keep it real-time.
- `std::time::Instant` panics on wasm32-unknown-unknown; all timing goes
  through `web_time` (std passthrough on native). Never reintroduce
  `std::time::Instant` in code shared with the browser build.
- File I/O (`write_png`, headless, dump button) is cfg-gated to native.
  Keep new fs/CLI code behind `#[cfg(not(target_arch = "wasm32"))]` and keep
  `cargo check --target wasm32-unknown-unknown` green.
- `wasm-bindgen-cli` must exactly match the wasm-bindgen crate version;
  `scripts/build-web.sh` reads the version from the lockfile and installs to
  match.
- The browser build is a web component: `WebHandle` in `src/web.rs` plus the
  custom element in `docs/app/magnetic-clock.js`. Attribute grammar equals
  the CLI grammar; the parsers are shared in `src/field.rs`. Setting magnets
  resets per-hand strength/shape, so the JS re-applies all attributes in
  ATTRS order on any change; keep that ordering.
- Each `<magnetic-clock>` element runs its own full sim and owns a WebGL
  context; a page with many instances multiplies CPU cost and hits the
  browser's context limit around a dozen.
- After changing any `#[wasm_bindgen]` signature, the JS glue in
  `docs/app/pkg/` is stale until the owner reruns `scripts/build-web.sh`.

## Findings from the neighbor-cap bias fix

- The chain pair force caps its neighbor count (chain_max_neighbors). When
  the cap binds, WHICH neighbors count depends on hash iteration order; the
  original raster-order scan (cells top-to-bottom, left-to-right) kept
  upper-left neighbors, giving capped runs a net force bias that visibly
  drifted bands toward the upper left (owner-found at cap ~12).
  `SpatialHash::for_near` now visits cells nearest ring first, so a binding
  cap keeps the closest neighbors: isotropic and physically right for a
  1/r^4 force. Keep any future capped/truncated neighbor loops
  distance-ordered, never scan-ordered.
- The fix re-baselined default dumps slightly (the cap binds inside dense
  clumps even at 48); character unchanged, verified visually 2026-07-15.
  Any low-cap experiment data from before the fix is contaminated by the
  drift artifact.

## Findings from the dt-convergence check

- The default physics step (dt = 1/30) under-resolves the stiff chain and
  repulsion dynamics enough to bias emergent pattern scales: the selected
  band wavelength at default dt is ~30% below its dt-converged value
  (converged for dt <= 1/120). Fine for the art; not fine for
  measurements. Quantitative pattern studies must set `--dt 0.008333` or
  finer and expect ~4x the runtime.
- Noise at fixed dt does NOT move the wavelength across a 256x effective-
  diffusion range, so the known D ~ noise^2*dt noise-model wart is not the
  mechanism; it is integrator overshoot in the deterministic forces.

## Decision history

- Motion trails / phosphor decay: rejected by the owner. The buffer clears
  fully every frame. Do not reintroduce trails as a "cheap improvement".
- Chain textures: explicitly requested by the owner; simulated with real pair
  forces, not faked with oriented strokes alone. See
  [design-simulation.md](design-simulation.md).
- Fluid solver: rejected during planning as needless complexity for the
  desired look.
