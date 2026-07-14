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

## Decision history

- Motion trails / phosphor decay: rejected by the owner. The buffer clears
  fully every frame. Do not reintroduce trails as a "cheap improvement".
- Chain textures: explicitly requested by the owner; simulated with real pair
  forces, not faked with oriented strokes alone. See
  [design-simulation.md](design-simulation.md).
- Fluid solver: rejected during planning as needless complexity for the
  desired look.
