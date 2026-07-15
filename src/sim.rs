//! Overdamped particle simulation. No inertia: each step computes velocities
//! from forces, then integrates positions at a fixed dt. Deterministic given
//! (seed, start time, step count). See agent_docs/design-simulation.md.

use rayon::prelude::*;

use crate::field::FieldSources;
use crate::vec2::Vec2;

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
    /// Soft-core repulsion range (also the spatial-hash cell size).
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
const W_DISP_SMOOTH: f64 = 0.2;

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
            chain_speed_cap: 0.12,
            chain_max_neighbors: 48,
            chain_cone: 0.0,
            field_clamp: crate::field::MIN_DIST,
            fluid_scale: 1.0,
            seed: 1,
        }
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

    fn cell_of(&self, p: Vec2) -> (i32, i32) {
        let gx = (((p.x + 1.0) / self.cell) as i32).clamp(0, self.dims - 1);
        let gy = (((p.y + 1.0) / self.cell) as i32).clamp(0, self.dims - 1);
        (gx, gy)
    }

    fn build(&mut self, pos: &[Vec2]) {
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
    fn for_near(&self, p: Vec2, k: i32, mut f: impl FnMut(usize)) {
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
    pub dir: Vec2,
    /// Moment saturation weight 0..=1 (|B|/b_sat capped); scales chaining.
    pub w: f64,
    /// Low-passed copy of `w` for rendering, so pointer press/release fades
    /// instead of flashing. Physics uses the instant `w`.
    pub w_disp: f64,
    /// Speed-capped magnetic velocity term.
    fv: Vec2,
}

pub struct Sim {
    pub params: SimParams,
    pub pos: Vec<Vec2>,
    /// Last step's velocities, kept for the velocity debug view.
    pub vel: Vec<Vec2>,
    /// Scratch buffer for the drag-coupling velocity smoothing pass.
    vel_smooth: Vec<Vec2>,
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
            pos.push(Vec2::new(a.cos() * r, a.sin() * r));
        }
        Self {
            vel: vec![Vec2::ZERO; params.count],
            vel_smooth: vec![Vec2::ZERO; params.count],
            field: vec![FieldSample::default(); params.count],
            field_scratch: vec![FieldSample::default(); params.count],
            step_index: 0,
            hash: SpatialHash::new(params.repulsion_radius),
            params,
            pos,
            rng,
        }
    }

    /// Live particle count change: truncate, or spawn new particles uniformly
    /// over the dish. Interactive-only; headless runs never call this, so
    /// dump determinism is unaffected.
    pub fn set_count(&mut self, n: usize) {
        self.params.count = n;
        if n <= self.pos.len() {
            self.pos.truncate(n);
            self.vel.truncate(n);
            self.vel_smooth.truncate(n);
            self.field.truncate(n);
            self.field_scratch.truncate(n);
            // The hash still holds dangling indices until the next step.
            self.hash.build(&self.pos);
        } else {
            while self.pos.len() < n {
                let a = self.rng.f64() * TAU;
                let r = self.rng.f64().sqrt() * DISH_R;
                self.pos.push(Vec2::new(a.cos() * r, a.sin() * r));
                self.vel.push(Vec2::ZERO);
                self.vel_smooth.push(Vec2::ZERO);
                self.field.push(FieldSample::default());
                self.field_scratch.push(FieldSample::default());
            }
        }
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
        // The hash cell size tracks the effective repulsion radius; rebuild
        // the grid when it changes live.
        if (self.hash.cell - p.repulsion_radius).abs() > f64::EPSILON {
            self.hash = SpatialHash::new(p.repulsion_radius);
        }
        self.hash.build(&self.pos);
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
        // Neighborhood must cover the widest active interaction.
        let range = if chains || coupling {
            p.chain_range.max(r_rep)
        } else {
            r_rep
        };
        let k_cells = ((range / r_rep).ceil() as i32).clamp(1, 4);

        // Pass 1: field samples. One analytic sweep gives B (for the induced
        // moment: superparamagnetic beads align with the local field,
        // saturating at b_sat) and grad(|B|^2) for the magnetic pull,
        // speed-capped. The cap is what makes the second hand outrun its
        // particles (the comet trail).
        self.pos
            .par_iter()
            .map(|&pos| {
                let (b, g) = sources.b_and_grad_b2(pos);
                let mut fv = g * p.mobility;
                let sp = fv.len();
                if sp > p.max_speed {
                    fv = fv * (p.max_speed / sp);
                }
                // Display/magnetization field: the force uses the full field,
                // but the pointer magnet's contribution is attenuated here or
                // it would saturate w and reorient every stroke dish-wide.
                let b = b - sources.pointer_b(pos) * (1.0 - p.pointer_visual);
                let bl = b.len();
                let w = (bl / p.b_sat).min(1.0);
                FieldSample {
                    dir: if bl > 1e-12 { b / bl } else { Vec2::ZERO },
                    w,
                    w_disp: w,
                    fv,
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
        self.vel.par_iter_mut().enumerate().for_each(|(i, vel)| {
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
            let mut rep = Vec2::ZERO;
            let mut cand: Vec<(usize, f64)> = Vec::with_capacity(64);
            hash.for_near(pos, k_cells, |j| {
                if j == i {
                    return;
                }
                let d = pos - positions[j];
                let dist = d.len();
                if dist <= 1e-9 || dist >= range {
                    return;
                }
                if dist < r_rep {
                    rep += (d / dist) * (1.0 - dist / r_rep);
                }
                if chains && dist < p.chain_range {
                    cand.push((j, dist));
                }
            });
            let cap = p.chain_max_neighbors as usize;
            if cand.len() > cap {
                cand.select_nth_unstable_by(cap, |a, b| a.1.total_cmp(&b.1));
                cand.truncate(cap);
            }
            let mut chain_v = Vec2::ZERO;
            for &(j, dist) in &cand {
                let w = wi * field[j].w;
                if w < 1e-3 {
                    continue;
                }
                // Attraction floor: bead spacing, tightened for strongly
                // magnetized pairs (field-dependent chain compression).
                let floor = p.chain_spacing * (1.0 - p.chain_compress * w);
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
                if cone_t > 0.0
                    && dist > 1.5 * p.chain_spacing
                    && bracket.dot(rh) < 0.0
                    && (mir * mir < cone_t || mjr * mjr < cone_t)
                {
                    continue;
                }
                let fall = (r_rep / dist).powi(4);
                chain_v += bracket * (p.chain_strength * w * fall);
            }
            v += rep * p.repulsion_strength;
            let cl = chain_v.len();
            if cl > p.chain_speed_cap {
                chain_v = chain_v * (p.chain_speed_cap / cl);
            }
            v += chain_v;

            // Dish wall.
            let rad = pos.len();
            if rad > DISH_R {
                v += pos.normalized() * (-(rad - DISH_R) * WALL_K);
            }

            // Brownian jitter from a stateless per-particle stream.
            let mut rng = Rng::new(noise_base ^ (i as u64).wrapping_mul(0xA24BAED4963EE407));
            let a = rng.f64() * TAU;
            v += Vec2::new(a.cos(), a.sin()) * p.noise;

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
                    let mut sum = Vec2::ZERO;
                    let mut wsum = 0.0;
                    hash.for_near(pos, k_cells, |j| {
                        if j == i {
                            return;
                        }
                        let dist = (pos - positions[j]).len();
                        if dist < range {
                            let w = 1.0 - dist / range;
                            sum += (vel[j] - vi) * w;
                            wsum += w;
                        }
                    });
                    *out = if wsum > 0.0 {
                        vi + sum * (p.drag_coupling / wsum)
                    } else {
                        vi
                    };
                });
            std::mem::swap(&mut self.vel, &mut self.vel_smooth);
        }

        // Pass 3: integrate.
        let dt = p.dt;
        self.pos.par_iter_mut().zip(&self.vel).for_each(|(pos, &v)| {
            let mut np = *pos + v * dt;
            // Backstop clamp; the wall force handles the normal case.
            let rad = np.len();
            let limit = DISH_R + 0.02;
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
        layouts: &[crate::field::HandMagnets; 3],
        t0: f64,
        seconds: f64,
    ) -> f64 {
        let steps = (seconds / self.params.dt).round() as u64;
        for k in 0..steps {
            let t = t0 + k as f64 * self.params.dt;
            let sources = FieldSources::at_time(layouts, t, self.params.field_clamp);
            self.step(&sources);
        }
        t0 + steps as f64 * self.params.dt
    }

    /// Pairs currently within chain interaction range (both magnetized),
    /// for the chains debug view.
    pub fn chain_bonds(&self) -> Vec<(Vec2, Vec2)> {
        let cut = self.params.chain_range * self.params.fluid_scale;
        let k = ((cut / (self.params.repulsion_radius * self.params.fluid_scale)).ceil() as i32)
            .clamp(1, 4);
        let mut out = Vec::new();
        for i in 0..self.pos.len() {
            if self.field[i].w < 0.15 {
                continue;
            }
            self.hash.for_near(self.pos[i], k, |j| {
                if j <= i || j >= self.pos.len() || self.field[j].w < 0.15 {
                    return;
                }
                if (self.pos[i] - self.pos[j]).len() < cut {
                    out.push((self.pos[i], self.pos[j]));
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
