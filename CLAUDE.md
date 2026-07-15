Guidance for agents working in this repo. Read this first, then the relevant
file in `agent_docs/`.

## What this is

A Rust + egui clock. The hands carry magnets (point dipoles, soft discs, or
bar magnets built from pole-face charges). Above them sits a simulated liquid
layer of magnetic particles in the overdamped regime: each fixed-dt step,
particle velocity comes from the analytic field gradient of the hand magnets
(plus an interactive pointer magnet), short-range dipole-dipole chaining,
soft-core repulsion, optional drag coupling, noise, and the dish wall.
Everything is rasterized into one CPU pixel buffer shown as an egui texture.
Ships as a native app (with headless PNG mode for verification) and as a
wasm `<magnetic-clock>` web component whose attributes reuse the CLI grammar.

All plan phases are built and owner-tuned; the plan was promoted to
[agent_docs/architecture.md](agent_docs/architecture.md).

## Layout

| Path          | Role                                                |
|---------------|-----------------------------------------------------|
| `src/`        | application code                                    |
| `agent_docs/` | architecture, design decisions, gotchas (below)     |
| `docs/`       | GitHub Pages site (index.html, img/), committed     |
| `scripts/`    | web build script; experiment analysis (numpy+PIL)   |
| `docs/app/`   | wasm build of the clock (pkg/ from build-web.sh)    |
| `docs/debug/` | dumped debug bitmaps, disposable, gitignored        |

## Commands

```bash
cargo run --release                 # interactive clock
cargo run --release -- --headless --time 10:08:30 --sim-seconds 60 --dump out.png
                                    # render offscreen, write PNG, exit (agent verification)
                                    # more flags: --view field,quiver,dipoles,velocity,hash,chains
                                    #   --particles N --seed N --size PX --stroke-len F
                                    #   --palette ice|ember|emerald|violet|mono --bg RRGGBB
                                    #   --max-px N (interactive resolution cap, 0 = off)
                                    #   --mobility F --max-speed F --noise F --repulsion F
                                    #   --magnets tip|strip:N|alt:N (one value, or hour,minute,second)
                                    #   --strengths F (one value, or hour,minute,second)
                                    #   --shapes point|disc:R|rect:FxW (one value, or hour,minute,second;
                                    #     F = bar length as fraction of hand length, 0..2, 1 = full hand)
                                    #   --chain-strength F --chain-spacing F --chain-range F
                                    #   --chain-compress F --chain-speed-cap F --chain-neighbors N
                                    #   --repulsion-radius F --dt F --field-clamp F --drag F
                                    #   --fluid-scale F (band-size dial; similarity transform
                                    #     of the microphysics, wavelength scales linearly)
                                    #   --pointer-strength F --pointer-radius F --pointer-visual F
                                    #     (touch/mouse magnet; visual = weight in stroke color)
magnetic-time --grad-check          # verify analytic field gradient vs numeric; run after
                                    # changing field elements (honors --magnets/--shapes)
                                    # headless two-phase runs (hysteresis experiments):
                                    #   --anneal-from F --anneal-for SECONDS
cargo check                         # compile check; do not run cargo test
cargo check --target wasm32-unknown-unknown   # browser build must stay green
./scripts/build-web.sh              # build wasm into docs/app/pkg/ (installs a
                                    # matching wasm-bindgen-cli; owner runs this)
```

Keep this block in sync with the CLI (USAGE in `src/main.rs` is the full
reference).

## Docs

- [agent_docs/architecture.md](agent_docs/architecture.md): module map, data flow, verification methodology, deferred work. Start here.
- [agent_docs/design-simulation.md](agent_docs/design-simulation.md): physics model: field elements, overdamped particles, chains, drag coupling, pointer magnet. Read before touching sim code.
- [agent_docs/design-rendering.md](agent_docs/design-rendering.md): pixel-buffer rendering, themes/palettes, debug views, headless PNG dump.
- [agent_docs/gotchas.md](agent_docs/gotchas.md): traps and decision history (numerics, egui, wasm, presets).
- [agent_docs/research-chain-banding.md](agent_docs/research-chain-banding.md): band physics, resolved: zippering builds the walls, tidal fragmentation spaces them; experiments, retractions, instruments.

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
- Never write meta-narrative sentences: no announcing what the text will do
  ("the short version", "deserves its own accounting", "the rest of this
  page..."). Start with the substance.
- State each rule on its own line as always/never.
- Mark inferred claims and open questions; don't present a guess as fact.
- Keep this file the routing entry point; subsystem detail goes in agent_docs/.
