#!/usr/bin/env bash
# Build the browser version into docs/app/ (served by GitHub Pages).
# Needs: rustup with the wasm32-unknown-unknown target, and wasm-bindgen-cli
# matching the wasm-bindgen version in Cargo.lock (installed below if missing
# or mismatched).
set -euo pipefail
# Run from the repo root regardless of where the script is invoked.
cd "$(dirname "$0")/.."

# cargo install puts binaries in ~/.cargo/bin, which is not on PATH on
# distro-packaged cargo setups.
export PATH="$HOME/.cargo/bin:$PATH"

rustup target add wasm32-unknown-unknown

# wasm-bindgen-cli must match the crate version exactly.
WB_VERSION=$(cargo pkgid wasm-bindgen | sed 's/.*[@#]//')
if ! command -v wasm-bindgen >/dev/null || [ "$(wasm-bindgen --version | awk '{print $2}')" != "$WB_VERSION" ]; then
    echo "installing wasm-bindgen-cli $WB_VERSION"
    cargo install wasm-bindgen-cli --version "$WB_VERSION" --locked
fi

cargo build --release --target wasm32-unknown-unknown

wasm-bindgen \
    --out-dir docs/app/pkg \
    --target web \
    --no-typescript \
    target/wasm32-unknown-unknown/release/magnetic-time.wasm

# Optional size pass if binaryen is installed.
if command -v wasm-opt >/dev/null; then
    wasm-opt -O2 -o docs/app/pkg/magnetic-time_bg.wasm docs/app/pkg/magnetic-time_bg.wasm
fi

du -h docs/app/pkg/*.wasm
echo "done: open docs/app/index.html via a local server, e.g.:"
echo "  python -m http.server -d docs"
