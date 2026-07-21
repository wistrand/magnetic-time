# Gotchas and findings

Non-obvious traps and decision history, in rough chronological order. Add
what implementation teaches; correct entries that turn out wrong.

## Numerics (confirmed in practice)

- Dipole fields diverge as 1/|r|^3 at the source. Evaluation distance is
  clamped (`SimParams::field_clamp`, default MIN_DIST in `src/field.rs`;
  discs clamp at their own radius); without it forces explode for particles
  that reach a magnet. Side effect: the field plateaus
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
- The |B|^2 gradient was forward-difference at this point (superseded: it is
  analytic now, see the next section).
- When benchmarking, build first; `time cargo run` after an edit measures
  the compile, not the sim.
- The per-particle chain-candidate Vec was the hot loop's only heap
  allocation (~0.8M allocs/s at the default preset). Moving it to a
  per-task scratch (rayon `for_each_init`) measured 0.5%, within noise,
  natively: glibc's thread arenas make same-size alloc/free nearly free,
  and the neighbor pass dominates anyway. Kept because it is free,
  dependency-less, and byte-identical; wasm (sequential rayon through
  dlmalloc) should benefit more, unmeasured. Do not spend further effort
  on allocator pressure here; smallvec in particular would heap-spill
  exactly in dense clumps where the loop is hottest.
- This machine's turbo makes cold runs ~35% faster than steady-state;
  interleave A/B binaries after a warmup run or the comparison measures
  thermals.

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

## Findings from the neighbor-cap bias fixes (two rounds)

- The chain pair force caps its neighbor count (chain_max_neighbors). When
  the cap binds, WHICH neighbors count must not depend on iteration order.
  Round 1 (owner-found at cap ~12): raster-order cell scanning kept
  upper-left neighbors and drifted bands up-left; fixed by visiting hash
  cells nearest ring first. Round 2 (owner-found at fluid_scale > 2, where
  the cap binds constantly): the residual raster order WITHIN each ring
  still biased upward and caused band oscillation; fixed properly by
  gathering all in-range candidates and truncating to the N NEAREST by
  distance (`select_nth_unstable_by` in the pass-2 loop). The rule, now
  twice-earned: capped neighbor sets must be distance-SELECTED, never
  order-truncated.
- Each fix re-baselined dense-clump behavior slightly (the cap binds there
  even at 48); character unchanged, verified visually. Capped-regime data
  predating 2026-07-15 nearest-N selection is contaminated at high
  fluid_scale or low caps.
- The neighbor search visits at most 4 hash-cell rings. Until 2026-07-15
  the cell size was hard-wired to repulsion_radius, so slider-reachable
  configs with chain_range/repulsion_radius > 4 (ratio up to 30) silently
  truncated chain and drag interactions at 4 cells, and the `--view
  chains` overlay clamped identically, hiding it. Fixed by growing the
  cell to range/4 when the ratio exceeds 4 (the k=4 visit bound stays;
  cost moves to scanning larger cells). Defaults are byte-identical
  (ratio 1.9); dumps from older builds at ratios > 4 are not comparable.

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

## Findings from the chain-length measurement (positions vs images)

- Image-based structure estimators on dot dumps (`--stroke-len 0`) fuse
  axially overlapping dots (bead spacing 4.5 px < dot diameter 5 px), so
  they systematically undercount along-field structure. An image-only
  pass concluded "no chains exist" in a regime where position data shows
  axial runs of 3-7+ beads. Measure structure from `--dump-positions`
  (positions + local field CSV), never from rendered pixels.
- Connected components of axial bonds percolate in dense zones: a
  bond-angle threshold keeps ~1/3 of bonds, and at ~9 bonds/particle
  that is far above the percolation threshold (one "chain" of 3834
  beads). Chain length must be measured by path tracing (best-aligned
  bond forward/backward per bead), not by component size.
- The 20-54.7 deg annulus of the dipole attraction cone is the restoring
  torque that keeps a bonded pair aligned against rotational diffusion.
  Cutting attraction outside a narrow axial cone for bonded pairs
  disintegrates every aggregate to single beads; the experimental
  `--chain-cone` gate therefore exempts bonded-range pairs and gates
  recruitment only (see the comment in `src/sim.rs`).

## Findings from input validation

- Three input paths set sim params, and they consume ONE bounds table:
  the `bounds` module in `src/sim.rs`, one `Bound` const per float field.
  Each `Bound` carries a hard-valid range (what can run) and an
  interactive range (a subset, for comfort). The CLI (`src/main.rs`)
  calls `Bound::validate` and hard-errors, because a user who typed
  `--dt 0` wants to be told; the web setters (`src/web.rs`) call
  `Bound::clamp` and the sliders (`src/app.rs`) call `Bound::ui`, both
  silent because a live control has no error channel. To change a limit,
  edit the const; never re-add a literal in a consumer. The interactive
  range is deliberately tighter than valid (e.g. fluid_scale valid > 0,
  interactive 0.1..8.0), so the CLI reaches values the sliders will not
  (verified: `--fluid-scale 12` runs).
- `Bound::clamp` is only called from the wasm-only `web.rs`, so it needs
  `#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]` or the
  native build warns (the mirror of main.rs's wasm dead-code allow).
- The CLI had NO range validation before 2026-07-15, only `.parse()` type
  checks. Two inputs crashed or hung rather than producing bad art: `dt
  <= 0` set the step count to `u64::MAX` via `(seconds/dt) as u64`
  (saturating cast), an effective hang; and any input driving the hash
  cell to 0 (`--fluid-scale 0`, `--repulsion-radius 0` with chains off)
  made `dims = (2.0/cell) as i32` overflow to `i32::MAX` and panicked in
  `for_near`. `validate` rejects both, plus NaN/inf (which parse from the
  CLI as "nan"/"inf" and would poison positions silently).
- Field-element flags (`--magnets`/`--shapes`/`--strengths`) were already
  safe from every path: their parsers in `src/field.rs` clamp (count
  1..16, length fraction 0..2, disc radius 0.005..0.3). Only the scalar
  sim params were unguarded.

## Findings from the alternative faces (seg, tide)

- One bar magnet per segment does NOT render a readable segment: a bar's only
  pole faces are its two ends, so particles pool at segment junctions and the
  interiors stay dark. Each segment is built from SEG_SUB collinear
  alternating bars so pole nodes distribute along it (see
  [design-simulation.md](design-simulation.md)). If a future change makes
  digits look like corner-dots, this is why.
- `--grad-check` on a seg face reports a higher mean relative error (~1e-2)
  than hands (~5e-4) with more >1% outliers. Not a bug: the seg face packs
  ~100-150 charge elements densely, so many random sample points land inside
  an element's r_min clamp where the numeric stencil straddles the kink (the
  analytic one-sided value is correct). The `expand` path is shared with
  hands and unchanged.
- Adding the `Face` enum refactored `FieldSources::at_time` to dispatch on
  face and share element expansion via `expand`. Verified behavior-preserving
  for hands by byte-identical headless dump against the pre-refactor baseline;
  keep that check when touching `expand` or `at_time`.
- Adding a new face: put its config in `FaceConfigs` (field.rs), add a
  `FaceKind` variant and a `FaceConfigs::build` arm, a `Face` variant, and
  the `at_time` + `draw_clock` match arms (the compiler lists both). The
  carrying structs (`Options`, `AppConfig`, `ClockApp`) hold one
  `FaceConfigs` and need no new fields; that grouping is why. Wiring the new
  face's knobs is still per-surface (CLI arm, web setter, JS attribute, dev
  slider). draw_clock's seg branch already draws any marker-emitting face
  from `sources.markers`, so a new one gets its overlay nearly free. Confirm
  hands and seg stay byte-identical after the change.
- Tide's growing arcs must be placed and switched by ARC LENGTH from the
  start angle, not by absolute angle. A bar's angle is `START + arc` where
  `START = -pi/2` (12 o'clock); testing the front with the absolute angle
  (`total - th`) instead of the arc length (`total - arc`) makes every arc
  ~90 degrees too long and, worse, leaves ~90 degrees of bars alive at the
  wrap where the ring should vanish (owner-reported: seconds magnets not
  disappearing at 59->00). The wrap is a deliberate discontinuity (arc resets
  to empty, whole ring gone in one step); every OTHER per-step change is
  smoothed by the fixed grid plus the fade-in leading edge, verified by a
  flat field-heatmap frame-to-frame sweep (ratio ~1.0 away from the wrap).

## Findings from the parallel rasterizer

- The particle draw pass was the last serial hot path (the sim was already
  rayon-parallel), and its cost scales with total stroke pixel area, so long
  strokes tanked FPS. `draw_particles` now bands the buffer
  (`par_chunks_mut`) and each band rasterizes particles clipped to its rows.
  Byte-exactness holds ONLY because each pixel is in exactly one band and
  particles run in index order per band, so the per-pixel blend order is
  unchanged; if you ever bin particles differently or reorder them, re-verify
  the byte-identical dump.
- The per-row x-span optimization in `raster_capsule` MUST stay a superset of
  covered pixels or output changes. It works because coverage needs distance
  to the segment < hw, and distance to the segment >= distance to the line, so
  every covered pixel is inside the infinite-line strip. Keep the per-pixel
  `if f > 0.0` guard: it trims the few extra strip pixels that are not covered,
  which is what makes the tightening exact rather than approximate. Do NOT
  swap the sqrt falloff for a squared-distance form to save a sqrt: it changes
  the anti-aliasing and breaks byte-exact dumps (left on the table
  deliberately).
- `rayon::current_num_threads()` returns 1 on wasm (sequential fallback), so
  the same `par_chunks_mut` path serves both; no cfg split is needed (an
  earlier version had one, removed as pointless).

## Findings from the f32 hybrid

- The sim is bandwidth-bound: per-particle state was 144 B (`Vec2` f64 pos +
  48 B `FieldSample`), ~3.9 MB at 27k, which blows the Pi 5's 2 MB L2, and the
  neighbor pass gathers scattered `pos[j]`/`field[j]`. So particle state moved
  to f32 (`Vec2f`, f32 `w`/`w_disp`), halving what the gather touches (144 ->
  ~72 B/particle, ~3.9 -> ~1.9 MB at 27k). Measured (`make bench`, min of 3):
  on a 16-core desktop it is ~neutral (ample cache), but on the Raspberry Pi 5
  (the cache-starved target) f32 vs f64 gained 1.2-1.6x, biggest where
  particle state dominates: dense 50k 18 -> 28 fps (1.57x), default 27k
  67 -> 85 (1.27x), render 69 -> 86 (1.25x), fine-dt 303 -> 366 (1.21x). tide
  gained least, 13 -> 14 fps (1.06x), because its bottleneck is the (still f64)
  field pass -- many magnet elements -- not particle bandwidth. The win tracks
  the hypothesis exactly: it lands where the neighbor gather is the cost.
- KEEP AoS, do not go struct-of-arrays. Splitting `Vec2` into separate `x[]`
  and `y[]` arrays would make each scattered neighbor gather touch two cache
  lines instead of one; f32-AoS (8 bytes, one read) is the sweet spot. The
  "SoA" in the old deferred note was wrong for a gather-bound loop.
- The field pass stays f64 on PURPOSE: `b_and_grad_b2` accumulates a Jacobian
  over many 1/r^3 elements with a large near-source dynamic range, and it is
  not the bottleneck. Particle `pos` is widened to f64 for the query
  (`pos.to_f64()`), computed in f64, then the result stored as f32. So
  `--grad-check` is UNCHANGED by this work (verified: 4.7e-4). If you ever
  push the field pass to f32, grad-check is the guard to watch.
- f32 breaks byte-exact dumps (rounding), a deliberate re-baseline like the
  rayon change; determinism WITHIN f32 is preserved (verified). Do not
  `cmp -s` sim dumps across this boundary; verify visually + grad-check.
- `Vec2` (f64, world/field/render/clock) and `Vec2f` (f32, particle state)
  are generated from one macro in vec2.rs so they cannot drift; convert only
  at the boundary with `to_f32`/`to_f64`. The hot loops use f32 copies of the
  `SimParams` constants (`*_32` locals in `step`), built once per step.

## Findings from spatial reordering

- `Sim::reorder` reindexes the particle arrays into Z-order (Morton) every
  `REORDER_EVERY` (16) steps so spatially-near particles are index-near and
  the neighbor gather reads mostly-contiguous memory. Cost is negligible
  (a sort + 4 permute-gathers every 16 steps, << 1% of step time), so worst
  case is neutral; disable with a large `REORDER_EVERY` (a one-line revert).
- Verlet neighbor lists were REJECTED first: per-step motion (~0.005-0.01) is
  a large fraction of the interaction range (~0.0228) at dt=1/30, and the
  rebuild is triggered by the fastest particle globally, so the list would
  rebuild almost every step (amortization ~1x). Verlet also would not fix the
  real cost (gathering scattered `pos[j]`), only the hash-navigation overhead.
  Reordering fixes the gather's memory pattern instead, and needs no skin.
- Measured FLAT on the 16-core desktop (1.0-1.02x; ample L3 hides it), same as
  f32. The Pi upside is real but smaller than first pitched, because f32
  ALREADY brought the default 27k working set (~1.9 MB) inside the Pi 5's 2 MB
  L2 -- once it fits L2, scattered vs contiguous access matters less. The
  remaining win is for HIGH particle counts (dense 50k ~3.6 MB still exceeds
  L2) and L1 locality. Measure on the Pi with `make bench` (especially dense)
  and keep it only if it helps there; it cannot hurt (negligible cost).
- Deterministic (stable sort by Morton key, fixed schedule), so dumps stay
  reproducible; but reindexing changes each particle's noise stream, so
  results diverge in fine detail from an un-reordered run (like rayon/f32).
- Reordering broke `set_count`'s count-down path (owner-found: lowering the
  particle count cleared whole regions). Once particles are in Morton order,
  the LAST n are a spatial region, so tail-truncation removes that region.
  Fixed by keeping a strided subset (`i * old / n`), evenly spaced along the
  Z-curve = spatially uniform. Any code that assumed particle index is
  spatially random is now wrong; reindexing correlates index with position.

## Findings on SIMD (why it is not the lever)

- SIMD is the wrong tool for the bottleneck. The neighbor pass is memory-gather
  and branch bound, not FLOP bound: it gathers scattered `pos[j]`/`field[j]`
  with data-dependent branches (r_rep, chain_range, cone gate, `w < 1e-3`) and
  a variable neighbor count. SIMD speeds arithmetic, so it does little here;
  the wins were memory (f32) and locality (reordering). The Pi 5's Cortex-A76
  has NEON but no SVE, so there is no hardware gather on the target either.
- The only real way to SIMD a neighbor pass is a GROMACS-style cluster kernel:
  group particles into fixed clusters of 4/8, compute every cluster-vs-cluster
  pair as a dense block, and MASK off the out-of-cutoff pairs, trading wasted
  FLOPs for branch-free, fully-packed, gather-free lanes (Pall & Hess 2013, the
  "Verlet cluster scheme"). It needs a per-ISA kernel set (SSE/AVX/AVX2/
  AVX-512/NEON) plus a cluster neighbor structure, and it fights our nearest-N
  cap (`select_nth`) and cone gate, which are data-dependent per-particle
  selections the uniform-block model does not accommodate. Disproportionate for
  a real-time desk clock; do not attempt.
- SIMD does apply to the arithmetic-bound SECONDARY costs, but both are niche
  or already handled: the field pass for field-heavy faces (tide: sqrt/div over
  hundreds of elements, but kept f64, so ~2-wide NEON, ~2x ceiling), and the
  rasterizer inner loop (already banded-parallel; the heatmap sidesteps it).
- Cheap options if ever pursued, in order: LLVM already auto-vectorizes the
  streaming passes (integrate), and f32 doubled that width (4xf32 on NEON/wasm
  vs 2xf64); enabling wasm SIMD for the browser is a build flag
  (`-C target-feature=+simd128`), no code, measure it; targeted NEON in the
  tide field pass only if tide-on-Pi becomes a requirement.
- Lineage: the perf thread walked up the MD-optimization ladder (spatial hash
  -> Verlet lists -> cluster kernels); this project lives on the first rung and
  correctly stops there. Both "Verlet lists" (rejected above) and GROMACS's
  cluster scheme are the same Verlet heritage.

## Findings from the palette redo (blend vs background)

- The particle blend has two directions: additive (dense marks climb toward
  white) and subtractive (dense marks darken toward black). Both encode the
  same thing: accumulation, so density reads as intensity. A plain
  source-over alpha blend would NOT accumulate (a 200-particle band would look
  like one stroke), which is why there are two modes rather than one.
- Accumulation only has headroom in one direction per background: you can add
  light onto dark, or subtract light from light, but not the reverse (adding
  bright ink onto a bright bg moves nothing; it saturates at 255 immediately).
  So the mode must match the ink-vs-bg contrast direction.
- The ORIGINAL rule keyed the mode to `Style::bg` luminance alone (dark bg ->
  additive). That was a hidden assumption that the palette is always a
  dim->bright ramp (true of the old named presets, all bright). Once the
  palette became a free `start`/`end` pair, bg luminance no longer predicts
  the ink direction: a bright preset on a light bg picked additive and the
  dense crests -- the whole point of the image -- blended into invisibility
  (verified: `--bg ece9e2 --palette ice`, band crests vanished).
- Fix: `Theme::ink_add` compares the palette `end` (the dense-crest color)
  against bg luminance -- additive if the ink is brighter, subtractive if
  darker. `end` is used (not `start` or a midpoint) because it is the color
  the densest marks converge to, and those are what must contrast. A palette
  that straddles the bg luminance has no single right answer, but that is an
  unusual palette; the common case (both endpoints one side of the bg) is
  exact.
- Face theming (dial, rim, ticks, hands) still keys off bg luminance
  (`Theme::dark`), correctly: those are not palette-colored, they just need to
  contrast with their own background.

## Decision history

- JSON presets (`src/preset.rs`) are hand-rolled flat JSON, not serde. The
  project stays dependency-light (rayon, chrono, png), so a flat one-level
  object (values are number/bool/string) keeps a ~50-line reader viable
  without a derive crate. Apply is lenient by design: unknown keys ignored,
  missing keys keep current values, numeric sim params clamped to `bounds`.
  Do not "upgrade" to serde or nested JSON without owner sign-off; the flat
  schema is also what makes hand-editing a preset safe. Round-trip
  (save -> load -> save) is byte-identical; keep it that way.
- Motion trails / phosphor decay: rejected by the owner. The buffer clears
  fully every frame. Do not reintroduce trails as a "cheap improvement".
- Chain textures: explicitly requested by the owner; simulated with real pair
  forces, not faked with oriented strokes alone. See
  [design-simulation.md](design-simulation.md).
- Fluid solver: rejected during planning as needless complexity for the
  desired look.
