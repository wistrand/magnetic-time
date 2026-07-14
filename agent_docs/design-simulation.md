# Design: simulation

Physics model for the particle layer. Everything here is a design decision made
during planning; once code exists, the code is authoritative for what happens,
this file for why.

## Model summary

Micron-scale magnetic particles in viscous liquid are in the overdamped (low
Reynolds) regime: inertia is negligible and Stokes drag dominates. So there is
no acceleration state; each step computes a velocity directly:

```
v_i = mobility * (F_field(x_i) + F_pair(i)) + advection(x_i) + brownian
x_i += v_i * dt        (dt fixed and clamped, speed capped; see gotchas)
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

Cost is dipole_count x particle_count per frame, parallelized with rayon.
If this ever limits particle count, the planned optimization is to precompute
each hand's `grad(|B|^2)` on a grid in the hand's local frame once at startup
(the layout is rigid), then rotate-and-sample per particle: cost becomes
independent of dipole count. Do not build this until profiling demands it.

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

## Secondary effects

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

## Open questions

- Saturation cap and mobility values: pure tuning, resolve in the tuning phase
  against the debug views.
- Whether chains need a small alignment torque term (rotating m_i toward B
  smoothly) or induced m_i = c*B is enough. Try the simple version first.
