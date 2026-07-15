# Research: bands perpendicular to chains

Why particles form bands running perpendicular to the chain direction, as
observed in dumps and interactively (2026-07). Mechanism analysis verified
against this codebase's pair force; literature references collected but not
figure-verified (see caveats).

## The mechanism: magic-angle staggering (zippering)

The dipole pair force in `Sim::step` (the `bracket` term) has radial
component `mm - 3*(mi.r_hat)*(mj.r_hat)` for the interacting pair. For
parallel moments at angle theta between the pair separation and the field:

- radial force is attractive for theta < 54.7 deg (more along-field),
- repulsive beyond it (side-by-side).

Verified: this sign structure is exactly present in our implemented force,
not just in the textbook energy it derives from.

Consequences:

1. Head-to-tail attraction strings beads into chains along field lines.
2. Two adjacent chains in bead-aligned registry repel (side-by-side).
3. Staggered by half a bead spacing, each bead sits diagonally next to its
   neighbor chain's gap, inside the attractive cone: chains lock laterally
   ("zippering").
4. Many chains in staggered registry form a 2D lattice. Its transverse rows,
   correlated bead positions across chains, are the perpendicular bands.

In the literature this is chain zippering / lateral coalescence, ending in
BCT columns in 3D; our 2D equivalent is a centered-rectangular lattice.
Stroke rendering amplifies the read: a transverse row of aligned strokes is
visually strong.

## What sets the spacings

| Knob               | Effect                                                        |
|--------------------|---------------------------------------------------------------|
| `chain_spacing`    | Along-chain bead period, so the band period along the field    |
| `repulsion_radius` | Lateral chain-to-chain spacing (soft-core floor)               |
| `chain_strength`, `b_sat` | Crystallization strength: low = loose fur, high = sharp lattice |
| `chain_compress`   | Field-graded spacing: bands tighten toward the magnets         |

## Caveats specific to this model

- The attraction floor (`chain_spacing`) prevents full zippering into
  contact; the lattice constant is floor-set, not energy-set. Real systems
  compress further with field strength; ours only does so via
  `chain_compress`.
- Moments are not globally parallel here: the field is the strongly
  non-uniform field of hand magnets, so the lattice axes curve with the
  local field. Expect bands that bend along |B| contours and converge
  toward tips, not straight stripes.
- The neighbor cap (CHAIN_MAX_NEIGHBORS) and summed chain speed cap damp
  crystallization in very dense clumps; the sharpest lattices form at
  moderate density.

## Competing explanations in our geometry (unresolved per-image)

Banding in a given dump can come from three sources that look alike:

1. Zippered crystallization (above): spacing tracks `repulsion_radius` and
   `chain_spacing`.
2. |B|-contour effects: accumulation shells at the r_min clamp plateau, and
   field nulls between alternating poles; these hug |B| isolines regardless
   of particle spacing.
3. Sweep-wake deposition: the second hand's bar deposits a furrow per lap
   (see the rings preset); band positions then track hand-lap history, not
   particle physics constants.

To attribute a specific image: vary `repulsion_radius` (moves type 1),
overlay `--view field,chains` at the same seed (type 2 follows the heatmap
contours), or change `--sim-seconds` by one second-hand lap (type 3 moves).
The rings preset's concentric rings are believed to be mostly type 3 with
type 1 texture inside each band (inferred, not yet tested).

## Experiments (proposed 2026-07, none run yet)

Cellular-automata-inspired questions, adapted to this system's ground rules:
bands are conserved matter (not information over a substrate), dynamics are
overdamped and near a gradient flow, the drive is external (the hands), and
runs are deterministic given (seed, params, time). All experiments fit the
existing tooling: same-seed headless runs at increasing `--sim-seconds` are
frame-exact movies, parameter sweeps cost seconds per run, the pointer
magnet is a perturbation probe, `--view chains` is the bond-level
microscope. The missing instrument is an order parameter extracted from
dumps (2D FFT: a zippered lattice gives a transverse Bragg-like peak, fur an
isotropic ring); that is an analysis script, not a sim change.

1. Zippering phase diagram. RUN 2026-07-15, results below. Sweep
   `chain_strength` x `noise` at fixed seed/time/count; classify each dump
   via the order parameter in `scripts/band_order.py` (needs numpy+PIL:
   blob-centered radial profile around the hour-tip pole, detrended
   autocorrelation peak at 18..60 px lags). Config that isolates zippering:
   time 09:00:00, second hand strength 0 (no wakes), full-length bars on
   hour/minute (`--magnets tip --shapes rect:1x0.03 --strengths 0.3,0.15,0`),
   `--mobility 2e-8 --max-speed 0.05`, 12000 particles, 180 sim-seconds,
   size 1000.

   Results (order parameter; gas baseline is ~0.04-0.10):

   - Banding requires chains. chain_strength 0 gives featureless radial fur
     at every noise level despite the identical field; this also settles
     attribution for pole rings: they are chain-driven, not |B|-contour
     packing.
   - Onset is below chain_strength 0.01: any sampled nonzero attraction
     produces rings (order ~0.15-0.37, coherent ~30-34 px period). The
     birth threshold at this mobility/density is essentially "any pair
     attraction at all".
   - Melting with noise, seed-averaged at chain_strength 0.03: order 0.25
     (noise 0.008, 3 seeds) -> 0.10 (noise 0.05). Gradual in this range,
     not a sharp line. Single-seed estimates scatter roughly +-0.1; average
     seeds before trusting a cell.
   - Owner preset chain params (0.01, noise 0.008) measure 0.26 with
     visibly soft rings: just above onset, modest order. Edge-of-order
     corollary weakly supported.
   - The visible band period is NOT the chain lattice constant. Doubling
     chain_spacing + chain_range moved the period only 32 -> 37 px (+16%),
     and an age series shows it is dynamics-set: condensation tightens
     (39 px at 90 s -> 32 px at 180 s), then coarsening/accretion widens
     spacing and consumes inner rings (at 360 s survivors sit 60-100 px
     apart around cleared zones). This revises the knob table above: chains
     supply the rigidity (necessary condition), but the wavelength is a
     coarsening scale, matching the literature's two-stage chains -> walls
     picture, not the primary lattice.

   Instrument notes for reruns: only use interior poles (the minute-tip
   annulus reaches the dial rim, which reads as a fake ring); lags below
   ~18 px are contaminated by stroke-length autocorrelation; the 60 px lag
   cap under-reads heavily coarsened states.
2. Birth/death threshold = nucleation vs spinodal. RUN 2026-07-15. The
   prediction (hysteretic) was WRONG on the evidence so far. Protocol: the
   headless `--anneal-from F --anneal-for SECONDS` flags run two-phase
   sims; down-branch = 180 s at chain_strength 0.06 (form bands) then 180 s
   at a final strength X, up-branch = 360 s straight at X, for
   X in {0, 0.002, 0.004, 0.008} (exp-1 config, noise 0.008).

   Results:

   - No order-parameter hysteresis: up- and down-branch order at each X
     agree within single-seed scatter (~+-0.1). The banding transition
     behaves as a continuous, reversible crossover, not a nucleated
     first-order one; there is no birth-rule gate.
   - Death at zero attraction is real but incomplete: after 180 s at
     chain_strength 0, the ring lattice has fully dissolved into fur
     (order collapses), BUT the annealed run retains large depleted halos
     and broad density undulations around the poles that the never-ordered
     run lacks. Order forgets faster than matter: pattern amnesia is
     incomplete. Ghost decay timescale is unmeasured (feeds experiment 6).
   - Morphology is history-dependent even where order is not: at equal
     final parameters, band periods differ between branches (fresh fine
     bands vs coarsened-then-dissolved texture).

   Caveats: single seed per cell; 180 s may undersample slow dissolution at
   intermediate X; conclusions hold at this mobility/density/config only.
3. Front propagation. In the quasi-static hour-hand region, track the
   ordered-region boundary across a same-seed time series. Predicted: initial
   speed set by mobility times pair force at `chain_range`, then stalls as
   the local particle reservoir depletes (supply-limited, unlike CA fronts).
4. Band collisions. Drive bands together (grad(|B|^2) compression of wake
   bands, or the pointer). Conservation of matter restricts outcomes to:
   merge (staggered registry), stack as adjacent lattice rows (aligned
   registry), or independent evaporation. Annihilation and pass-through are
   impossible (no momentum, conserved particles); observing either would
   mean a bug.
5. Organisms.
   - Loops: chains closing along the field lines that loop pole-to-pole on a
     bar magnet; the spiral curls at overhung pole ends in the rings preset
     are candidates. Inspect with `--view chains`.
   - Gliders: the second-hand comet already qualifies (shape persists while
     constituent particles turn over), and the second hand is a glider gun
     emitting one wake band per lap.
   - Autonomous oscillators: near-gradient-flow dynamics should forbid them;
     any oscillation must inherit its clock from the hands or noise. Caveat:
     speed caps and XSPH coupling break exact gradient structure, so a limit
     cycle is unlikely but not provably excluded. Finding one would be a
     real result.
   - Knots: excluded in 2D.
6. History dependence. Determinism: same (seed, params, time) is
   byte-identical, by construction. Chaos: same coarse pattern via different
   micro-history diverges (documented when the gradient scheme changed).
   Material memory: the dish encodes drive history (wake bands = recent
   laps; hysteresis = parameter path). Quantifiable follow-up: memory
   capacity and decay, i.e. how many laps back are reconstructable from one
   frame before coarsening erases them.
7. Coarsening exponents. Does band count decay as a power law in time?
   Track the FFT peak across a dump sequence; compare against zippering
   literature scaling. Partial data from experiment 1's age series
   (90/180/360 s): non-monotone period (condensation then coarsening) and
   inner-ring consumption by the poles; a proper exponent needs a denser
   time series and a wider autocorrelation window than
   `scripts/band_order.py` currently uses.
8. Template reproduction. Seed one band next to uniform gas (pointer);
   does it accrete an adjacent row (autocatalytic lateral growth)? This is
   the system's closest analogue to reproduction.

## References (collected 2026-07, from abstracts/snippets; automated
## full-text fetch was blocked, so figure-level claims are unverified)

- Soft Matter review (2025), self-assembly of magnetic colloids under
  unsteady fields: chains "zipper together", networks coarsen into bands
  perpendicular to the field.
  sciencedirect.com/science/article/pii/S1359029425000093
- Vega-Bellido et al., reversible zippering of chains in magnetic
  nanofluids. researchgate.net/publication/38081855
- Morphology of anisotropic chains in an MR fluid (microscopy of chains
  merging into columns). arxiv.org/pdf/cond-mat/0701239
- Nonequilibrium cluster structures in a thin MR layer, DC field (closest
  geometry to our 2D dish). pubs.aip.org/aip/adv/article/10/5/055012
- Aligned colloidal clusters, BCT crystals (PNAS 2024).
  pnas.org/doi/10.1073/pnas.2404145121
- Ferrofluid film stripe/labyrinth patterns (macroscopic form).
  sciencedirect.com/science/article/abs/pii/S030488530000826X
- CC-licensed real photos: Wikimedia Commons, Category:Ferrofluids.
