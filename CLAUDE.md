Guidance for agents working in this repo. Read this first, then the relevant
file in `agent_docs/`.

## What this is

A Rust + egui desktop clock. The hands carry magnets (point dipoles, soft
discs, or bar magnets built from pole-face charges). Above them sits a simulated liquid layer of magnetic particles
in the overdamped regime: each frame, particle velocity is computed from the
field gradient of the hand magnets plus short-range dipole-dipole interaction
(chain formation) plus drag, noise, and boundary forces. Particles are
rasterized into a CPU pixel buffer shown as an egui texture; the clock face and
hands are egui vector shapes.

Phases 1-4 of [agent_docs/plan.md](agent_docs/plan.md) are built; tuning
(phase 5) is ongoing with the owner.

## Layout

| Path          | Role                                                |
|---------------|-----------------------------------------------------|
| `src/`        | application code                                    |
| `agent_docs/` | plan, design decisions, gotchas (linked below)      |
| `docs/`       | GitHub Pages site (index.html, img/), committed     |
| `scripts/`    | owner-run helper scripts (web build)                |
| `docs/app/`   | wasm build of the clock (pkg/ from build-web.sh)    |
| `docs/debug/` | dumped debug bitmaps, disposable, gitignored        |

## Commands

```bash
cargo run --release                 # interactive clock
cargo run --release -- --headless --time 10:08:30 --sim-seconds 60 --dump out.png
                                    # render offscreen, write PNG, exit (agent verification)
                                    # more flags: --view field,quiver,dipoles,velocity,hash,chains
                                    #   --particles N --seed N --size PX --stroke-len F
                                    #   --palette ice|ember|emerald|violet|mono
                                    #   --mobility F --max-speed F --noise F --repulsion F
                                    #   --magnets tip|strip:N|alt:N (one value, or hour,minute,second)
                                    #   --strengths F (one value, or hour,minute,second)
                                    #   --chain-strength F --chain-spacing F --chain-range F
                                    #   --chain-compress F --drag F
magnetic-time --grad-check          # verify analytic field gradient vs numeric; run after
                                    # changing field elements (honors --magnets/--shapes)
                                    #   --shapes point|disc:R|rect:FxW (one value, or hour,minute,second;
                                    #     F = bar length as fraction of hand length, 0..2, 1 = full hand)
cargo check                         # compile check; do not run cargo test
cargo check --target wasm32-unknown-unknown   # browser build must stay green
./scripts/build-web.sh              # build wasm into docs/app/pkg/ (installs a
                                    # matching wasm-bindgen-cli; owner runs this)
```

Headless flags are the planned interface; keep this block in sync when the CLI
lands.

## Docs

- [agent_docs/plan.md](agent_docs/plan.md): phased build plan and status. Start here for any implementation work.
- [agent_docs/design-simulation.md](agent_docs/design-simulation.md): physics model: dipole field, overdamped particles, chain formation. Read before touching sim code.
- [agent_docs/design-rendering.md](agent_docs/design-rendering.md): pixel-buffer rendering, debug views, headless PNG dump.
- [agent_docs/gotchas.md](agent_docs/gotchas.md): known traps (numeric stability, egui performance).

## Invariants

- The particle buffer is fully cleared every frame. Never add decay, motion
  trails, or phosphor effects; the owner explicitly rejected them.
- All time flows from one clock source with a speed multiplier. Never read wall
  time anywhere else in sim or rendering.
- Physics steps use a fixed, clamped dt decoupled from frame rate. Frame rate
  must never change simulation outcomes.
- Headless dump and interactive mode share the same simulation and
  rasterization path, so dumped bitmaps are faithful to what the user sees.
- Particle interactions are cutoff-limited and use the spatial hash. Never
  introduce an all-pairs O(N²) loop.

## Conventions

- Verify changes visually: run the headless dump and read the PNG. No test
  suite; do not add one unasked.
- Rust 2021+, rustfmt defaults. Do not run formatters or linters unasked.
- Keep sim constants as named tunables in one place, exposed in the dev slider
  panel, not scattered literals.

## Documentation Style

- Markdown links for doc references an agent should follow, not backticks.
  Backticks are for source paths and inline code. Align table columns.
- No AI-isms (no "powerful", "seamlessly", "leverage", rule-of-three, "not just
  X but Y"). No em dashes or emojis in project copy. State the point directly.
- Concise; assume the agent is competent. Add only what it can't infer.
- State each rule on its own line as always/never.
- Mark inferred claims and open questions; don't present a guess as fact.
- Keep this file the routing entry point; subsystem detail goes in agent_docs/.
