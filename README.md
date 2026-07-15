# magnetic-time

A desktop clock where the hands carry magnets and move beneath a thin layer of
liquid filled with magnetic particles. The particles are dragged along by the
field of the hands, lagging with viscous drag: slow hands carry their
particles, the second hand outruns its own and plows rings, wakes, and comet
trails that slowly relax. Every pattern on the dial is simulated, not painted.

Native Rust application rendered with egui; also builds to WebAssembly as a
`<magnetic-clock>` web component. Magnet layout, shape, and strength are
configurable per hand (point dipoles, discs, bar magnets), with chain physics
and drag coupling tunables, five color palettes, light/dark backgrounds with
adaptive ink rendering, and touch/mouse interaction (drag a disc magnet
through the particles).

![The default preset: concentric particle rings on a dark dial](docs/img/rings.png)

More screenshots, a longer description, and a live in-browser build (wasm):
[project page](https://wistrand.github.io/magnetic-time/)
(source in [docs/](docs/), publishable via GitHub Pages; rebuild the wasm app
with `./scripts/build-web.sh`). The stripe patterns turned out to be real,
testable physics and got their own investigation:
[the bands are objects, not waves](https://wistrand.github.io/magnetic-time/banding.html).

## Quick start

```bash
cargo run --release
```

The dev side panel exposes all tunables live (magnet layout/shape/strength per
hand, particle physics, time-speed multiplier). Headless rendering to PNG:

```bash
cargo run --release -- --headless --time 13:37:35 --sim-seconds 600 --dump out.png
```

See `cargo run -- --help` for all flags.

## Development

Agent-oriented docs live in [CLAUDE.md](CLAUDE.md) and
[agent_docs/](agent_docs/); start with
[agent_docs/architecture.md](agent_docs/architecture.md).

## License

[MIT](LICENSE)
