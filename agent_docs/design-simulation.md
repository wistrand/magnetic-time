# Design: simulation

Physics model for the particle layer. Everything here is a design decision made
during planning; once code exists, the code is authoritative for what happens,
this file for why.

## Model summary

Micron-scale magnetic particles in viscous liquid are in the overdamped (low
Reynolds) regime: inertia is negligible and Stokes drag dominates. So there is
no acceleration state; each step computes a velocity directly:

```
v_i = mobility * grad(|B|^2)(x_i)   (speed-capped)
    + v_pair(i)                     (chain attraction + soft-core repulsion,
                                     direct velocity scales)
    + brownian + wall
x_i += v_i * dt        (fixed dt; per-term speed caps; see gotchas)
```

A full fluid solver (Navier-Stokes, ferrofluid magnetization) was considered
and rejected: the overdamped particle model produces the desired look at a
fraction of the complexity, and the piece is judged by look, not physical
accuracy.

## Hand magnets and the driving field

Each hand carries a rigid layout of point dipoles (position along the hand,
moment vector, polarity). Layouts are data-driven so they can be swapped per
hand: tip-only magnet vs an alternating-polarity strip give visibly different
particle signatures (single blob vs stripes).

Field of one dipole at offset r: `B(r) = k * (3(m.r_hat)r_hat - m) / |r|^3`.
Total B is the sum over all field elements of all hands.

Magnets have a shape (`MagnetShape` in `src/field.rs`), which changes the
field, not just the marker:

- Point: ideal dipole, near field clamped at MIN_DIST.
- Disc: same dipole far field (exact for a uniformly magnetized disc), near
  field clamped at the disc radius instead, giving a large soft capture zone
  with no singular core.
- Rect: a bar magnet as two pole faces of distributed magnetic charge (a few
  point charges per face, `q * r_hat / r^2` each), total charge scaled so the
  far field matches a point dipole of the same strength. Near the magnet the
  field is face-shaped: flat approach zones at the poles, weak flanks.

A superparamagnetic bead is pulled toward high field magnitude regardless of
sign: `F_field ∝ grad(|B|^2)`. Polarity still shapes the pattern through the
structure of |B| (alternating poles create field nulls between magnets).
B and grad(|B|^2) are computed analytically in one sweep
(`FieldSources::b_and_grad_b2`: accumulate B and the Jacobian, gradient =
2 J^T B). After any field-element change, run `--grad-check`, which compares
against a numeric reference.

Cost is element_count x particle_count per step, parallelized with rayon.
Before optimizing here, note the profiling finding in
[gotchas.md](gotchas.md): at the owner presets the sim is neighbor-bound
(chain/repulsion pass), not field-bound. If field cost ever dominates, the
planned optimization is to precompute each hand's field on a grid in the
hand's local frame once at startup (the layout is rigid), then
rotate-and-sample per particle.

## Drag is the aesthetic core

The mobility-to-hand-speed ratio decides the character of each hand:

- Hour hand: particles keep up; tight tracking cluster.
- Minute hand: mostly keeps up, slight smear.
- Second hand: tip outruns particle terminal velocity; particles form a comet
  trail that relaxes after the hand passes.

This ratio is the primary tunable. All such constants live in one tunables
struct exposed as dev sliders.

## Chain formation (required feature)

The owner wants visible chain textures, so chains are simulated, not faked:

- Each particle is superparamagnetic: induced moment `m_i = c * B(x_i)`,
  capped at a saturation magnitude.
- Particles within `chain_range` interact with the point-dipole pair force:
  head-to-tail attraction along the local field direction, side-by-side
  repulsion. This is what strings beads into chains along field lines.
- The attraction turns off below `chain_spacing`, which therefore sets the
  bead spacing (chain texture scale). `chain_compress` shrinks that floor for
  strongly magnetized pairs, so chains tighten near the magnets the way real
  chains compress in stronger fields.
- A soft-core repulsion at very short range gives particles finite size and
  prevents collapse into a point at field maxima.
- Pair contributions per particle are capped at the N nearest neighbors
  (`chain_max_neighbors`, distance-selected; see the neighbor-cap entries
  in [gotchas.md](gotchas.md) for why order-based truncation is forbidden).

Neighbor search uses a uniform spatial hash grid rebuilt each frame; the same
grid serves both pair forces and soft-core repulsion. Interactions are
cutoff-limited; a global all-pairs loop is forbidden (invariant in
[../CLAUDE.md](../CLAUDE.md)).

Rendering supports chains too: particles draw as short strokes aligned with
the local field direction rather than dots, which makes chains readable even
at modest particle counts. See
[design-rendering.md](design-rendering.md).

Rejected alternative: orientation-only fake chains (aligned strokes with no
pair force). Cheaper, but chains would not connect, bend around field lines,
or break when hands pass, and the owner asked for the real texture.

## Fluid scale (the band-size dial)

`SimParams::fluid_scale` applies a similarity transform to the particle
microphysics: all micro-lengths (repulsion_radius, chain_spacing,
chain_range) and micro-velocities (chain_strength, repulsion_strength,
noise, chain_speed_cap) are multiplied together, preserving every
dimensionless ratio that governs melting, chaining, and texture. The band
wavelength scales linearly with it (validated 0.5x..2x; mechanism: the
tidal-fragmentation scale (cs*r_rep^4/mu)^(1/5), see
[research-chain-banding.md](research-chain-banding.md)). Not scaled, by
design: the field, dish, hand geometry, mobility, max_speed, dt, and
field_clamp; consequently the clamp-adjacent zone near poles does not
shrink below scale ~0.5. Physically it selects a coarser or finer fluid:
at the owner's 20 cm dish target, scale 1 means ~1.2 mm repulsion cores
and ~7.5 mm bands.

## Secondary effects

- Pointer magnet (touch/mouse): a soft charge appended to the field while
  the pointer is down. Its force uses the full field, but its contribution
  to the display/magnetization field (stroke color, orientation, chain
  weight rendering) is attenuated by `pointer_visual`. This split is
  deliberate: F ~ grad(|B|^2) forces the pointer's |B| to exceed b_sat
  dish-wide at useful strengths, which would flash every stroke white and
  point it at the finger. The rendered weight `w_disp` is also low-passed
  (W_DISP_SMOOTH in `src/sim.rs`) so press/release fades instead of
  flashing. Do not "simplify" these back to the raw field.
- Drag coupling (`drag_coupling`, 0 = off): XSPH-style velocity smoothing
  after the force pass; each particle's velocity blends toward the
  kernel-weighted mean of its neighbors' within `chain_range`. Models
  momentum exchange through the liquid: clusters move cohesively and sweeping
  magnets produce coherent ripples instead of choppy fragments. This is the
  approximation of inter-particle hydrodynamics; there is still no bulk flow.
- Brownian noise: small random velocity per step, for texture and to break
  symmetry. Deterministic RNG seeded from a config value.
- Stirring advection: a moving hand drags nearby liquid. Modeled as a
  tangential velocity kernel around each hand scaled by its angular speed.
  Optional knob, off by default until tuning.
- Boundary: circular dish; particles are pushed back inside with a soft normal
  force (no hard reflection, it looks wrong under drag).

## Time

One clock source produces "display time" from wall time and a speed multiplier
(needed to watch minute/hour-hand behavior without waiting). Hands are smooth,
not ticking; a ticking second hand fights the fluid look. Sim steps at fixed
dt in display-time units, decoupled from frame rate.

## Resolved questions

- Saturation and mobility values: tuned by the owner; the current preset is
  baked into the `Default` impls (`SimParams`, `default_specs`, `Style`) and
  marked as owner-tuned in comments. Change only with the owner.
- Alignment torque for chains: not needed; instant induced moments
  (m_i along local B) produce convincing chains.
