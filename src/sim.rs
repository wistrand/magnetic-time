//! Overdamped particle simulation. No inertia: each step computes velocities
//! from forces, then integrates positions at a fixed dt. Deterministic given
//! (seed, start time, step count). See agent_docs/design-simulation.md.

use rayon::prelude::*;

use crate::field::FieldSources;
use crate::vec2::{Vec2, Vec2f};

const TAU: f64 = std::f64::consts::TAU;

/// Dish radius in clock-face units; particles live inside this.
pub const DISH_R: f64 = 0.92;

/// Inward wall push per unit of overlap, 1/s.
const WALL_K: f64 = 5.0;

#[derive(Clone, Copy)]
pub struct SimParams {
    pub count: usize,
    /// Fixed physics step, display seconds.
    pub dt: f64,
    /// Velocity per unit of grad(|B|^2).
    pub mobility: f64,
    /// Cap on the magnetic term's speed, units/s. Sets what a particle can
    /// chase: below second-hand tip speed, above minute-hand tip speed.
    pub max_speed: f64,
    /// Brownian speed, units/s.
    pub noise: f64,
    /// Soft-core repulsion range. Also the spatial-hash cell size, except
    /// that the cell grows to chain_range/4 when the range/repulsion ratio
    /// exceeds the 4-ring neighbor search (see the cell-size rule in
    /// `step`).
    pub repulsion_radius: f64,
    /// Repulsion speed at full overlap, units/s.
    pub repulsion_strength: f64,
    /// Dipole-dipole pair interaction scale, units/s. 0 disables chaining.
    pub chain_strength: f64,
    /// |B| at which induced moments saturate; chaining fades below this.
    pub b_sat: f64,
    /// Chain bead spacing, dial units: the pair attraction turns off below
    /// this distance and soft-core repulsion sets the equilibrium there.
    pub chain_spacing: f64,
    /// Chain pair-force cutoff distance, dial units. Also the drag-coupling
    /// kernel radius, and it sizes the neighbor search.
    pub chain_range: f64,
    /// 0..=1: how much the spacing floor shrinks for fully magnetized pairs.
    /// Real chains compress in stronger fields; 0 = fixed spacing.
    pub chain_compress: f64,
    /// 0..=1: XSPH-style velocity smoothing toward the neighborhood mean.
    /// Models short-range momentum exchange through the liquid; clusters move
    /// cohesively and moving magnets entrain nearby particles. 0 = off.
    pub drag_coupling: f64,
    /// Interactive pointer magnet (touch/mouse drag) charge; 0 disables.
    /// Charges fall off as 1/r^2 vs dipole 1/r^3, so useful values are much
    /// larger than hand strengths.
    pub pointer_strength: f64,
    /// Pointer magnet disc radius (near-field softness), dial units.
    pub pointer_radius: f64,
    /// How much of the pointer's field enters the display/magnetization
    /// field (0..=1). Force always uses the full field; without attenuation
    /// the pointer saturates stroke color and orientation across the whole
    /// dish (its 1/r^2 magnitude exceeds b_sat everywhere at useful
    /// strengths).
    pub pointer_visual: f64,
    /// Repel particles from the pointer instead of attracting them (a charge
    /// can only attract, so repel is a direct outward push; see
    /// `FieldSources::pointer_repel_grad`).
    pub pointer_repel: bool,
    /// Cap on the summed chain velocity per particle, units/s (stability).
    /// This, not max_speed, is the speed limit of zippering dynamics.
    pub chain_speed_cap: f64,
    /// Max chain pair contributions per particle per step; in a dense clump
    /// the force saturates anyway and this bounds the cost.
    pub chain_max_neighbors: u32,
    /// EXPERIMENTAL probe, not physics: half-angle in degrees that chain
    /// attraction is restricted to around the moment axis, applied only to
    /// recruitment (pairs beyond 1.5 chain spacings); 0 = off (the physical
    /// dipole cone, 54.7 deg). Added 2026-07-15 to test whether off-axis
    /// (staggered) capture is what keeps chainlets at ~2 beads: narrowing
    /// the cone should let chains grow axially if so.
    pub chain_cone: f64,
    /// Near-field clamp radius for point/rect field elements, dial units
    /// (passed to FieldSources::at_time; discs use their own radius if
    /// larger). Exposed 2026-07-15 to test whether the band wavelength is
    /// seeded by this scale.
    pub field_clamp: f64,
    /// Fluid coarseness: a similarity transform of the particle
    /// microphysics. Scales all micro-lengths (repulsion_radius,
    /// chain_spacing, chain_range) and micro-velocities (chain_strength,
    /// repulsion_strength, noise, chain_speed_cap) together, preserving
    /// every dimensionless ratio. The band wavelength scales linearly with
    /// it (tidal-fragmentation scale delta* ~ (cs*r_rep^4/mu)^(1/5); see
    /// research-chain-banding.md).
    pub fluid_scale: f64,
    pub seed: u64,
}

/// Per-step low-pass factor for the display weight `w_disp`; press/release
/// fades over roughly 5 steps (~1/6 display-second) instead of flashing.
const W_DISP_SMOOTH: f32 = 0.2;

/// Reorder particles into spatial (Z-order) order every this many steps, so
/// spatially-near particles stay index-near and the neighbor gather reads
/// contiguous memory. See `Sim::reorder`.
const REORDER_EVERY: u64 = 16;

/// Interleave the low 16 bits of `n` with zero bits (Morton "Part1By1").
fn part1by1(mut n: u32) -> u32 {
    n &= 0x0000_ffff;
    n = (n | (n << 8)) & 0x00ff_00ff;
    n = (n | (n << 4)) & 0x0f0f_0f0f;
    n = (n | (n << 2)) & 0x3333_3333;
    n = (n | (n << 1)) & 0x5555_5555;
    n
}

/// Z-order (Morton) code of a grid cell, for spatial-locality sorting.
fn morton(gx: u32, gy: u32) -> u32 {
    part1by1(gx) | (part1by1(gy) << 1)
}

// Defaults are the owner-tuned "rings" preset from 2026-07-14
// (screenshot-approved, full-length bar magnets); change only with the owner.
impl Default for SimParams {
    fn default() -> Self {
        Self {
            count: 27000,
            dt: 1.0 / 30.0,
            mobility: 2e-9,
            max_speed: 0.12,
            noise: 0.008,
            repulsion_radius: 0.012,
            repulsion_strength: 0.025,
            chain_strength: 0.01,
            b_sat: 3.0,
            // Spacing/range/compress/coupling defaults reproduce the pre-
            // parameterized behavior exactly (0.8 and 1.9 x repulsion_radius,
            // no compression, no coupling), preserving the rings preset.
            chain_spacing: 0.0096,
            chain_range: 0.0228,
            chain_compress: 0.0,
            drag_coupling: 0.0,
            pointer_strength: 30.0,
            pointer_radius: 0.05,
            pointer_visual: 0.03,
            pointer_repel: false,
            chain_speed_cap: 0.12,
            chain_max_neighbors: 48,
            chain_cone: 0.0,
            field_clamp: crate::field::MIN_DIST,
            fluid_scale: 1.0,
            seed: 1,
        }
    }
}

/// Single owner of every tunable's limits. Each [`bounds::Bound`] is used
/// three ways from one definition: the CLI rejects out-of-range input
/// ([`Bound::validate`]), and the web setters and dev-panel sliders clamp
/// and display within the interactive range ([`Bound::clamp`], [`Bound::ui`]).
/// The interactive range is a subset of the valid range: sliders stay in a
/// comfortable band while the CLI can still reach the full runnable range.
pub mod bounds {
    use std::ops::RangeInclusive;

    pub struct Bound {
        /// Hard valid minimum (what the CLI accepts).
        lo: f64,
        /// Is `lo` itself valid? false = strictly greater (e.g. dt > 0,
        /// where 0 hangs or panics). true = >= (negative is meaningless
        /// but 0 runs).
        lo_incl: bool,
        /// Hard valid maximum; `f64::INFINITY` = unbounded above.
        hi: f64,
        /// Interactive clamp and slider range, a subset of `[lo, hi]`.
        ui_lo: f64,
        ui_hi: f64,
    }

    impl Bound {
        /// CLI: reject non-finite or out-of-range, with a flag-named message.
        pub fn validate(&self, flag: &str, v: f64) -> Result<(), String> {
            let in_lo = if self.lo_incl { v >= self.lo } else { v > self.lo };
            if v.is_finite() && in_lo && v <= self.hi {
                return Ok(());
            }
            let lo = if self.lo_incl {
                format!(">= {}", self.lo)
            } else {
                format!("> {}", self.lo)
            };
            let hi = if self.hi.is_finite() {
                format!(" and <= {}", self.hi)
            } else {
                String::new()
            };
            Err(format!("{flag} must be a finite value {lo}{hi}, got {v}"))
        }

        /// Web/slider: force into the interactive range (NaN -> lower bound,
        /// since `f64::clamp` cannot order NaN). Only the wasm build clamps;
        /// the native CLI validates instead.
        #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
        pub fn clamp(&self, v: f64) -> f64 {
            if v.is_nan() {
                self.ui_lo
            } else {
                v.clamp(self.ui_lo, self.ui_hi)
            }
        }

        /// Slider min..=max.
        pub fn ui(&self) -> RangeInclusive<f64> {
            self.ui_lo..=self.ui_hi
        }
    }

    const INF: f64 = f64::INFINITY;
    /// `>= 0`, unbounded above, with a slider band.
    const fn non_neg(ui_lo: f64, ui_hi: f64) -> Bound {
        Bound { lo: 0.0, lo_incl: true, hi: INF, ui_lo, ui_hi }
    }
    /// `> 0`, unbounded above, with a slider band.
    const fn positive(ui_lo: f64, ui_hi: f64) -> Bound {
        Bound { lo: 0.0, lo_incl: false, hi: INF, ui_lo, ui_hi }
    }
    /// `0..=1` fraction.
    const fn unit(ui_hi: f64) -> Bound {
        Bound { lo: 0.0, lo_incl: true, hi: 1.0, ui_lo: 0.0, ui_hi }
    }

    pub const MOBILITY: Bound = non_neg(1e-10, 1e-6);
    pub const MAX_SPEED: Bound = non_neg(0.005, 0.3);
    pub const NOISE: Bound = non_neg(0.0, 0.05);
    pub const REPULSION_STRENGTH: Bound = non_neg(0.0, 0.3);
    pub const REPULSION_RADIUS: Bound = positive(0.002, 0.05);
    pub const CHAIN_STRENGTH: Bound = non_neg(0.0, 0.15);
    pub const B_SAT: Bound = positive(1.0, 2000.0);
    pub const CHAIN_SPACING: Bound = non_neg(0.002, 0.04);
    pub const CHAIN_RANGE: Bound = non_neg(0.005, 0.06);
    pub const CHAIN_COMPRESS: Bound = unit(1.0);
    pub const CHAIN_CONE: Bound = non_neg(0.0, 54.7);
    pub const CHAIN_SPEED_CAP: Bound = non_neg(0.005, 0.5);
    pub const DT: Bound = positive(0.004, 0.1);
    pub const FIELD_CLAMP: Bound = positive(0.005, 0.08);
    pub const FLUID_SCALE: Bound = positive(0.1, 8.0);
    pub const DRAG_COUPLING: Bound = unit(1.0);
    pub const POINTER_STRENGTH: Bound = non_neg(0.0, 150.0);
    pub const POINTER_RADIUS: Bound = positive(0.005, 0.5);
    pub const POINTER_VISUAL: Bound = unit(1.0);
}

impl SimParams {
    /// Reject values that would crash, hang, or make the sim degenerate.
    /// The CLI calls this after parsing and hard-errors with the message
    /// (`src/main.rs`). Bounds live in [`bounds`]; the web setters and
    /// sliders clamp to the same definitions instead of erroring, because a
    /// live control has no error channel. NaN/inf are rejected here too
    /// (they parse from the CLI as "nan"/"inf" and would poison positions).
    pub fn validate(&self) -> Result<(), String> {
        if self.count == 0 {
            return Err("--particles must be >= 1".to_string());
        }
        bounds::MOBILITY.validate("--mobility", self.mobility)?;
        bounds::MAX_SPEED.validate("--max-speed", self.max_speed)?;
        bounds::NOISE.validate("--noise", self.noise)?;
        bounds::REPULSION_STRENGTH.validate("--repulsion", self.repulsion_strength)?;
        bounds::REPULSION_RADIUS.validate("--repulsion-radius", self.repulsion_radius)?;
        bounds::CHAIN_STRENGTH.validate("--chain-strength", self.chain_strength)?;
        bounds::B_SAT.validate("b_sat", self.b_sat)?;
        bounds::CHAIN_SPACING.validate("--chain-spacing", self.chain_spacing)?;
        bounds::CHAIN_RANGE.validate("--chain-range", self.chain_range)?;
        bounds::CHAIN_COMPRESS.validate("--chain-compress", self.chain_compress)?;
        bounds::CHAIN_CONE.validate("--chain-cone", self.chain_cone)?;
        bounds::CHAIN_SPEED_CAP.validate("--chain-speed-cap", self.chain_speed_cap)?;
        bounds::DT.validate("--dt", self.dt)?;
        bounds::FIELD_CLAMP.validate("--field-clamp", self.field_clamp)?;
        bounds::FLUID_SCALE.validate("--fluid-scale", self.fluid_scale)?;
        bounds::DRAG_COUPLING.validate("--drag", self.drag_coupling)?;
        bounds::POINTER_STRENGTH.validate("--pointer-strength", self.pointer_strength)?;
        bounds::POINTER_RADIUS.validate("--pointer-radius", self.pointer_radius)?;
        bounds::POINTER_VISUAL.validate("--pointer-visual", self.pointer_visual)?;
        Ok(())
    }
}

/// SplitMix64: tiny deterministic RNG, no dependency.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Uniform grid over the dish; linked-list buckets rebuilt each step.
struct SpatialHash {
    cell: f64,
    dims: i32,
    heads: Vec<i32>,
    next: Vec<i32>,
}

impl SpatialHash {
    fn new(cell: f64) -> Self {
        let dims = (2.0 / cell).ceil() as i32;
        Self {
            cell,
            dims,
            heads: vec![-1; (dims * dims) as usize],
            next: Vec::new(),
        }
    }

    fn cell_of(&self, p: Vec2f) -> (i32, i32) {
        // Bin in f64 so the cell index is stable; positions are f32.
        let gx = (((p.x as f64 + 1.0) / self.cell) as i32).clamp(0, self.dims - 1);
        let gy = (((p.y as f64 + 1.0) / self.cell) as i32).clamp(0, self.dims - 1);
        (gx, gy)
    }

    fn build(&mut self, pos: &[Vec2f]) {
        self.heads.fill(-1);
        self.next.resize(pos.len(), -1);
        for (i, p) in pos.iter().enumerate() {
            let (gx, gy) = self.cell_of(*p);
            let c = (gy * self.dims + gx) as usize;
            self.next[i] = self.heads[c];
            self.heads[c] = i as i32;
        }
    }

    /// Visit all particles within `k` cells of p's cell (including p itself;
    /// the caller filters). Cells are visited nearest ring first (Chebyshev
    /// distance 0, 1, .., k): callers that cap the number of contributions
    /// (the chain pair force) then keep the closest neighbors instead of a
    /// scan-order-biased subset. A raster-order scan here caused capped
    /// runs to drift bands toward the upper left (owner-reported bug).
    fn for_near(&self, p: Vec2f, k: i32, mut f: impl FnMut(usize)) {
        let (cx, cy) = self.cell_of(p);
        for ring in 0..=k {
            for gy in (cy - ring).max(0)..=(cy + ring).min(self.dims - 1) {
                for gx in (cx - ring).max(0)..=(cx + ring).min(self.dims - 1) {
                    if (gy - cy).abs().max((gx - cx).abs()) != ring {
                        continue;
                    }
                    let mut j = self.heads[(gy * self.dims + gx) as usize];
                    while j >= 0 {
                        f(j as usize);
                        j = self.next[j as usize];
                    }
                }
            }
        }
    }

    /// Particle count per cell, for the hash debug view.
    fn cell_counts(&self) -> Vec<(i32, i32, u32)> {
        let mut out = Vec::new();
        for gy in 0..self.dims {
            for gx in 0..self.dims {
                let mut n = 0;
                let mut j = self.heads[(gy * self.dims + gx) as usize];
                while j >= 0 {
                    n += 1;
                    j = self.next[j as usize];
                }
                if n > 0 {
                    out.push((gx, gy, n));
                }
            }
        }
        out
    }
}

/// Per-particle field data, computed once per step in a parallel pass.
#[derive(Clone, Copy, Default)]
pub struct FieldSample {
    /// Induced moment direction (unit local B), for chaining and strokes.
    /// f32: the field is queried in f64 but stored/consumed in f32 (see
    /// vec2.rs; the neighbor pass gathers this per interaction).
    pub dir: Vec2f,
    /// Moment saturation weight 0..=1 (|B|/b_sat capped); scales chaining.
    pub w: f32,
    /// Low-passed copy of `w` for rendering, so pointer press/release fades
    /// instead of flashing. Physics uses the instant `w`.
    pub w_disp: f32,
    /// Speed-capped magnetic velocity term.
    fv: Vec2f,
}

pub struct Sim {
    pub params: SimParams,
    pub pos: Vec<Vec2f>,
    /// Last step's velocities, kept for the velocity debug view.
    pub vel: Vec<Vec2f>,
    /// Scratch buffer for the drag-coupling velocity smoothing pass.
    vel_smooth: Vec<Vec2f>,
    /// Per-particle field samples from the last step.
    pub field: Vec<FieldSample>,
    /// Scratch for pass 1 so the previous step's `w_disp` stays readable
    /// during the parallel fill.
    field_scratch: Vec<FieldSample>,
    /// Steps taken; also the noise stream selector, so noise is deterministic
    /// and independent of thread scheduling.
    step_index: u64,
    rng: Rng,
    hash: SpatialHash,
}

impl Sim {
    /// Particles start uniformly distributed over the dish.
    pub fn new(params: SimParams) -> Self {
        let mut rng = Rng::new(params.seed);
        let mut pos = Vec::with_capacity(params.count);
        for _ in 0..params.count {
            let a = rng.f64() * TAU;
            let r = rng.f64().sqrt() * DISH_R;
            pos.push(Vec2::new(a.cos() * r, a.sin() * r).to_f32());
        }
        Self {
            vel: vec![Vec2f::ZERO; params.count],
            vel_smooth: vec![Vec2f::ZERO; params.count],
            field: vec![FieldSample::default(); params.count],
            field_scratch: vec![FieldSample::default(); params.count],
            step_index: 0,
            hash: SpatialHash::new(params.repulsion_radius),
            params,
            pos,
            rng,
        }
    }

    /// Live particle count change: drop a spatially-uniform subset, or spawn
    /// new particles uniformly over the dish. Interactive-only; headless runs
    /// never call this, so dump determinism is unaffected.
    pub fn set_count(&mut self, n: usize) {
        self.params.count = n;
        let old = self.pos.len();
        if n < old {
            // The arrays are in spatial (Morton) order after `reorder`, so
            // truncating the tail would clear a whole region. Keep a strided
            // subset instead (evenly spaced along the Z-curve = spatially
            // uniform).
            let pick: Vec<usize> = (0..n).map(|i| i * old / n).collect();
            self.pos = pick.iter().map(|&j| self.pos[j]).collect();
            self.vel = pick.iter().map(|&j| self.vel[j]).collect();
            self.vel_smooth = pick.iter().map(|&j| self.vel_smooth[j]).collect();
            self.field = pick.iter().map(|&j| self.field[j]).collect();
            self.field_scratch.truncate(n); // scratch, rebuilt each step
            // The hash still holds dangling indices until the next step.
            self.hash.build(&self.pos);
        } else if n > old {
            while self.pos.len() < n {
                let a = self.rng.f64() * TAU;
                let r = self.rng.f64().sqrt() * DISH_R;
                self.pos.push(Vec2::new(a.cos() * r, a.sin() * r).to_f32());
                self.vel.push(Vec2f::ZERO);
                self.vel_smooth.push(Vec2f::ZERO);
                self.field.push(FieldSample::default());
                self.field_scratch.push(FieldSample::default());
            }
        }
    }

    /// Permute the particle arrays into Z-order of a coarse grid (cell size
    /// `cell`) so spatially-near particles are index-near. The neighbor gather
    /// then reads mostly-contiguous memory, which helps most in dense clumps
    /// and on cache-starved targets (the Pi). Deterministic (stable sort by
    /// Morton key), so dumps stay reproducible; but reindexing changes each
    /// particle's noise stream, so results diverge from an un-reordered run
    /// (fine detail only, like the rayon and f32 changes).
    fn reorder(&mut self, cell: f64) {
        let n = self.pos.len();
        if n < 2 {
            return;
        }
        let keys: Vec<u32> = self
            .pos
            .iter()
            .map(|&p| {
                let gx = (((p.x as f64 + 1.0) / cell) as i64).clamp(0, 0xffff) as u32;
                let gy = (((p.y as f64 + 1.0) / cell) as i64).clamp(0, 0xffff) as u32;
                morton(gx, gy)
            })
            .collect();
        let mut order: Vec<u32> = (0..n as u32).collect();
        order.sort_by_key(|&i| keys[i as usize]);
        // Gather the permutation into fresh buffers (rare: every
        // REORDER_EVERY steps). pos/vel/field carry per-particle state that
        // must stay together; field_scratch is rebuilt each step, skip it.
        self.pos = order.iter().map(|&i| self.pos[i as usize]).collect();
        self.vel = order.iter().map(|&i| self.vel[i as usize]).collect();
        self.vel_smooth = order.iter().map(|&i| self.vel_smooth[i as usize]).collect();
        self.field = order.iter().map(|&i| self.field[i as usize]).collect();
    }

    /// One fixed-dt step against the given field sources. Three parallel
    /// passes: field samples, then velocities from current positions, then
    /// integrate. Result is independent of particle order and thread
    /// scheduling (noise streams are keyed by particle index and step).
    pub fn step(&mut self, sources: &FieldSources) {
        let mut p = self.params;
        // Fluid coarseness: similarity-transform the microphysics (all
        // micro-lengths and micro-velocities together) so the pattern
        // rescales without changing character. See SimParams::fluid_scale.
        let fs = p.fluid_scale;
        if fs != 1.0 {
            p.repulsion_radius *= fs;
            p.chain_spacing *= fs;
            p.chain_range *= fs;
            p.chain_strength *= fs;
            p.repulsion_strength *= fs;
            p.noise *= fs;
            p.chain_speed_cap *= fs;
        }
        let r_rep = p.repulsion_radius;
        let chains = p.chain_strength > 0.0;
        let coupling = p.drag_coupling > 0.0;
        // Experimental cone gate (see SimParams::chain_cone): cos^2 of the
        // half-angle; attractive pairs whose separation axis falls outside
        // the cone (for either moment) are dropped. Dimensionless, so no
        // fluid_scale term.
        let cone_t = if p.chain_cone > 0.0 {
            p.chain_cone.to_radians().cos().powi(2)
        } else {
            0.0
        };
        // Neighborhood must cover the widest active interaction. The search
        // visits at most 4 cell rings, so when range/repulsion exceeds 4
        // (reachable from the sliders) the cell grows to range/4 instead of
        // silently truncating the interaction: cost then scales with cell
        // area, the honest price of a wide range.
        let range = if chains || coupling {
            p.chain_range.max(r_rep)
        } else {
            r_rep
        };
        let cell = r_rep.max(range / 4.0);
        // Rebuild the grid when the cell size changes live.
        if (self.hash.cell - cell).abs() > f64::EPSILON {
            self.hash = SpatialHash::new(cell);
        }
        // Periodically reindex particles into spatial order so the neighbor
        // gather below reads contiguous memory (see reorder()).
        if self.step_index % REORDER_EVERY == 0 {
            self.reorder(cell);
        }
        self.hash.build(&self.pos);
        let k_cells = ((range / cell).ceil() as i32).clamp(1, 4);

        // f32 copies of the constants the particle-space passes (2, 2.5, 3)
        // use, so the hot loop is pure f32 (particle state is f32; the field
        // pass below stays f64). See vec2.rs.
        let r_rep32 = r_rep as f32;
        let range32 = range as f32;
        let chain_range32 = p.chain_range as f32;
        let chain_spacing32 = p.chain_spacing as f32;
        let chain_compress32 = p.chain_compress as f32;
        let chain_strength32 = p.chain_strength as f32;
        let repulsion_strength32 = p.repulsion_strength as f32;
        let chain_speed_cap32 = p.chain_speed_cap as f32;
        let noise32 = p.noise as f32;
        let drag_coupling32 = p.drag_coupling as f32;
        let cone_t32 = cone_t as f32;
        let dish_r32 = DISH_R as f32;
        let wall_k32 = WALL_K as f32;
        let tau32 = TAU as f32;
        let dt32 = p.dt as f32;

        // Pass 1: field samples. One analytic sweep gives B (for the induced
        // moment: superparamagnetic beads align with the local field,
        // saturating at b_sat) and grad(|B|^2) for the magnetic pull,
        // speed-capped. The cap is what makes the second hand outrun its
        // particles (the comet trail).
        self.pos
            .par_iter()
            .map(|&pos| {
                // Query the field in f64 (accurate near sources; not the hot
                // pass), then narrow the per-particle result to f32.
                let pos = pos.to_f64();
                let (b, g) = sources.b_and_grad_b2(pos);
                // Repelling pointer: an outward push scaled like the field
                // gradient (zero in attract mode; that pointer is in `g`).
                let mut fv = (g + sources.pointer_repel_grad(pos)) * p.mobility;
                let sp = fv.len();
                if sp > p.max_speed {
                    fv = fv * (p.max_speed / sp);
                }
                // Display/magnetization field: the force uses the full field,
                // but the pointer magnet's contribution is attenuated here or
                // it would saturate w and reorient every stroke dish-wide.
                let b = b - sources.pointer_b(pos) * (1.0 - p.pointer_visual);
                let bl = b.len();
                let w = (bl / p.b_sat).min(1.0) as f32;
                FieldSample {
                    dir: if bl > 1e-12 { (b / bl).to_f32() } else { Vec2f::ZERO },
                    w,
                    w_disp: w,
                    fv: fv.to_f32(),
                }
            })
            .collect_into_vec(&mut self.field_scratch);

        // Carry the low-passed display weight over from the previous step.
        self.field
            .par_iter_mut()
            .zip(&self.field_scratch)
            .for_each(|(old, new)| {
                let w_disp = old.w_disp + (new.w - old.w_disp) * W_DISP_SMOOTH;
                *old = FieldSample { w_disp, ..*new };
            });

        // Pass 2: velocities.
        let (positions, field, hash) = (&self.pos, &self.field, &self.hash);
        let noise_base = p.seed ^ self.step_index.wrapping_mul(0xD1B54A32D192ED03);
        // Chain-candidate scratch is per rayon task (for_each_init), not per
        // particle: a fresh Vec here is ~0.8M allocations/s at the default
        // preset.
        self.vel.par_iter_mut().enumerate().for_each_init(
            || Vec::with_capacity(64),
            |cand: &mut Vec<(usize, f32)>, (i, vel)| {
            let pos = positions[i];
            let mut v = field[i].fv;

            // Neighbor forces: soft-core repulsion (never speed-capped; it
            // must win locally or clumps collapse) plus the dipole-dipole
            // pair force that strings beads into chains along field lines.
            // Chain candidates are gathered first and truncated to the N
            // NEAREST when over the cap: distance-selected truncation is
            // isotropic and stable, where visit-order truncation drifted
            // bands toward the scan direction (owner-found at high
            // fluid_scale, where the cap binds constantly).
            let (mi, wi) = (field[i].dir, field[i].w);
            let mut rep = Vec2f::ZERO;
            cand.clear();
            hash.for_near(pos, k_cells, |j| {
                if j == i {
                    return;
                }
                let d = pos - positions[j];
                let dist = d.len();
                if dist <= 1e-9 || dist >= range32 {
                    return;
                }
                if dist < r_rep32 {
                    rep += (d / dist) * (1.0 - dist / r_rep32);
                }
                if chains && dist < chain_range32 {
                    cand.push((j, dist));
                }
            });
            let cap = p.chain_max_neighbors as usize;
            if cand.len() > cap {
                cand.select_nth_unstable_by(cap, |a, b| a.1.total_cmp(&b.1));
                cand.truncate(cap);
            }
            let mut chain_v = Vec2f::ZERO;
            for &(j, dist) in cand.iter() {
                let w = wi * field[j].w;
                if w < 1e-3 {
                    continue;
                }
                // Attraction floor: bead spacing, tightened for strongly
                // magnetized pairs (field-dependent chain compression).
                let floor = chain_spacing32 * (1.0 - chain_compress32 * w);
                if dist < floor {
                    continue;
                }
                let d = pos - positions[j];
                let rh = d / dist;
                let mj = field[j].dir;
                let (mir, mjr, mm) = (mi.dot(rh), mj.dot(rh), mi.dot(mj));
                // Point dipole-dipole force direction on i (r_hat points
                // j -> i): head-to-tail attracts, side-by-side repels.
                let bracket = mi * mjr + mj * mir + rh * (mm - 5.0 * mir * mjr);
                // Cone gate on RECRUITMENT only (dist > 1.5 spacings): new
                // arrivals must approach within +-chain_cone of the moment
                // axis. Bonded-range pairs (axial and stagger) keep full
                // physics; gating them too deletes the restoring torque of
                // the 20-54.7 deg attraction annulus and bonds evaporate by
                // rotational diffusion (measured 2026-07-15: aggregates
                // disintegrate to single beads). Repulsion is never gated.
                if cone_t32 > 0.0
                    && dist > 1.5 * chain_spacing32
                    && bracket.dot(rh) < 0.0
                    && (mir * mir < cone_t32 || mjr * mjr < cone_t32)
                {
                    continue;
                }
                let fall = (r_rep32 / dist).powi(4);
                chain_v += bracket * (chain_strength32 * w * fall);
            }
            v += rep * repulsion_strength32;
            let cl = chain_v.len();
            if cl > chain_speed_cap32 {
                chain_v = chain_v * (chain_speed_cap32 / cl);
            }
            v += chain_v;

            // Dish wall.
            let rad = pos.len();
            if rad > dish_r32 {
                v += pos.normalized() * (-(rad - dish_r32) * wall_k32);
            }

            // Brownian jitter from a stateless per-particle stream.
            let mut rng = Rng::new(noise_base ^ (i as u64).wrapping_mul(0xA24BAED4963EE407));
            let a = rng.f64() as f32 * tau32;
            v += Vec2f::new(a.cos(), a.sin()) * noise32;

            *vel = v;
        });

        // Pass 2.5: drag coupling (XSPH velocity smoothing). Each particle's
        // velocity moves toward the kernel-weighted mean of its neighbors',
        // modeling momentum exchange through the liquid.
        if coupling {
            let (positions, hash, vel) = (&self.pos, &self.hash, &self.vel);
            self.vel_smooth
                .par_iter_mut()
                .enumerate()
                .for_each(|(i, out)| {
                    let pos = positions[i];
                    let vi = vel[i];
                    let mut sum = Vec2f::ZERO;
                    let mut wsum = 0.0f32;
                    hash.for_near(pos, k_cells, |j| {
                        if j == i {
                            return;
                        }
                        let dist = (pos - positions[j]).len();
                        if dist < range32 {
                            let w = 1.0 - dist / range32;
                            sum += (vel[j] - vi) * w;
                            wsum += w;
                        }
                    });
                    *out = if wsum > 0.0 {
                        vi + sum * (drag_coupling32 / wsum)
                    } else {
                        vi
                    };
                });
            std::mem::swap(&mut self.vel, &mut self.vel_smooth);
        }

        // Pass 3: integrate.
        self.pos.par_iter_mut().zip(&self.vel).for_each(|(pos, &v)| {
            let mut np = *pos + v * dt32;
            // Backstop clamp; the wall force handles the normal case.
            let rad = np.len();
            let limit = dish_r32 + 0.02;
            if rad > limit {
                np = np * (limit / rad);
            }
            *pos = np;
        });

        self.step_index += 1;
    }

    /// Advance the sim from display time `t0` over `seconds`, rebuilding the
    /// field each step as the hands move. Returns the end time.
    pub fn advance(
        &mut self,
        face: &crate::field::Face,
        t0: f64,
        seconds: f64,
    ) -> f64 {
        let steps = (seconds / self.params.dt).round() as u64;
        for k in 0..steps {
            let t = t0 + k as f64 * self.params.dt;
            let sources = FieldSources::at_time(face, t, self.params.field_clamp);
            self.step(&sources);
        }
        t0 + steps as f64 * self.params.dt
    }

    /// Pairs currently within chain interaction range (both magnetized),
    /// for the chains debug view.
    pub fn chain_bonds(&self) -> Vec<(Vec2, Vec2)> {
        let cut = self.params.chain_range * self.params.fluid_scale;
        // The step() cell-size rule guarantees cut/cell <= 4 while chains
        // are active, so the view covers the true interaction range.
        let k = ((cut / self.hash.cell).ceil() as i32).clamp(1, 4);
        let cut32 = cut as f32;
        let mut out = Vec::new();
        for i in 0..self.pos.len() {
            if self.field[i].w < 0.15 {
                continue;
            }
            self.hash.for_near(self.pos[i], k, |j| {
                if j <= i || j >= self.pos.len() || self.field[j].w < 0.15 {
                    return;
                }
                if (self.pos[i] - self.pos[j]).len() < cut32 {
                    out.push((self.pos[i].to_f64(), self.pos[j].to_f64()));
                }
            });
        }
        out
    }

    pub fn hash_cells(&self) -> Vec<(i32, i32, u32)> {
        self.hash.cell_counts()
    }

    pub fn hash_dims(&self) -> i32 {
        self.hash.dims
    }
}
