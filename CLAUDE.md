Guidance for agents working in this repo. Read this first, then the relevant
file in `agent_docs/`.

## What this is

A Rust + egui clock. The hands carry magnets (point dipoles, soft discs, or
bar magnets built from pole-face charges). The face is one of rotating hands
(default), a digital seven-segment readout whose lit segments are bar magnets
switched by the time (`--face seg`), or the tide arcs: three concentric arcs
of bar magnets that fill with the seconds, minutes, and hours (`--face tide`). Above them sits a simulated
liquid layer of magnetic particles in the overdamped regime: each fixed-dt
step, particle velocity comes from the analytic field gradient of the face
magnets (plus interactive pointer magnets, one per touch), short-range
dipole-dipole chaining, soft-core repulsion, optional drag coupling, noise,
an optional periodic disturbance burst, and the dish wall. The dish is a
parametric SDF shape (circle default; squares, superellipses, stars,
polygons, rings, holes via `--dish`).
Everything is rasterized into one CPU pixel buffer shown as an egui texture.
Ships as a native app (with headless PNG mode for verification, plus kiosk
options for wall displays: fullscreen, view rotation/mirroring, autosave)
and as a wasm `<magnetic-clock>` web component whose attributes reuse the
CLI grammar.

All plan phases are built and owner-tuned; the plan was promoted to
[agent_docs/architecture.md](agent_docs/architecture.md).

## Layout

| Path          | Role                                                |
|---------------|-----------------------------------------------------|
| `src/`        | application code                                    |
| `bin/`        | launch scripts (kiosk start, autosave clean)        |
| `agent_docs/` | architecture, design decisions, gotchas (below)     |
| `docs/`       | GitHub Pages site (index.html, img/), committed     |
| `scripts/`    | web build, perf benchmark (bench.sh), experiment analysis (numpy+PIL) |
| `docs/app/`   | wasm build of the clock (pkg/ from build-web.sh)    |
| `docs/debug/` | dumped debug bitmaps, disposable, gitignored        |

## Commands

A `Makefile` wraps the common ones (`make help` lists them: run, build,
check, check-wasm, web, grad-check, dump, bench, clean; fmt/clippy are
deliberate). `make bench` runs `scripts/bench.sh` (headless perf on a few
configs: min-of-N wall time plus an approx fps = fixed-dt sim frames per
wall-second; `RUNS=N` to change run count).
Pass flags with `make run ARGS="--face tide --fps"`. The raw commands:

```bash
cargo run --release                 # interactive clock
cargo run --release -- --headless --time 10:08:30 --sim-seconds 60 --dump out.png
                                    # render offscreen, write PNG, exit (agent verification)
                                    # --dump-positions out.csv: also write particle
                                    #   positions + local field (measurement scripts;
                                    #   image-based estimators fuse overlapping dots)
magnetic-time --grad-check          # verify analytic field gradient vs numeric; run after
                                    # changing field elements (honors --magnets/--shapes)
cargo check                         # compile check; do not run cargo test
cargo check --target wasm32-unknown-unknown   # browser build must stay green
./scripts/build-web.sh              # build wasm into docs/app/pkg/ (installs a
                                    # matching wasm-bindgen-cli; owner runs this)
```

USAGE in `src/main.rs` is the flag reference (sim tunables, faces/magnets,
window/kiosk modes, presets, autosave, palettes, debug views); keep it in
sync when changing the CLI. `bin/` has kiosk launch and autosave-clean
scripts.

## Docs

- [agent_docs/architecture.md](agent_docs/architecture.md): module map, data flow, verification methodology, deferred work. Start here.
- [agent_docs/design-simulation.md](agent_docs/design-simulation.md): physics model: field elements, overdamped particles, chains, drag coupling, pointer magnets, disturbance bursts, dish boundary. Read before touching sim code.
- [agent_docs/design-rendering.md](agent_docs/design-rendering.md): pixel-buffer rendering, statics cache, view transforms, themes/palettes, dev panel, debug views, headless PNG dump.
- [agent_docs/gotchas.md](agent_docs/gotchas.md): traps and decision history (numerics, egui input ownership, wasm, presets, caches, benchmarking).
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
