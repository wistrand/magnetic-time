#!/usr/bin/env bash
# Headless performance benchmark. Times the release binary on a few
# representative configs and reports the MIN wall time over RUNS runs (min is
# the stable estimator on a turbo-boosting CPU; see agent_docs/gotchas.md).
# Whole-process wall (init + sim + one render + PNG encode), so it is a coarse
# regression signal, not a per-frame profile. Lower is better.
#
#   ./scripts/bench.sh          # 3 runs each
#   RUNS=8 ./scripts/bench.sh   # more runs, less noise
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=target/release/magnetic-time
if [ ! -x "$BIN" ]; then
  echo "build first: cargo build --release (or make build)" >&2
  exit 1
fi

RUNS=${RUNS:-3}
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

bench() {
  local name="$1"
  shift
  "$BIN" "$@" --dump "$TMP/out.png" >/dev/null 2>&1 # warmup (thermals + cache)
  local times=()
  for _ in $(seq "$RUNS"); do
    local s e
    s=$(date +%s.%N)
    "$BIN" "$@" --dump "$TMP/out.png" >/dev/null 2>&1
    e=$(date +%s.%N)
    times+=("$(awk "BEGIN{print $e - $s}")")
  done
  printf '%s\n' "${times[@]}" |
    awk -v n="$name" -v r="$RUNS" 'NR==1||$1<m{m=$1} END{printf "  %-12s %7.3f s  (min of %d)\n", n, m, r}'
}

echo "headless benchmark: $(nproc) cores, $RUNS runs each, min wall time (lower is better)"
# Rings preset, moderate load (the CLAUDE.md sample render).
bench default --headless --time 10:08:30 --sim-seconds 30 --size 800
# Sim-bound: many particles, small buffer.
bench dense --headless --time 10:08:30 --sim-seconds 12 --size 800 --particles 50000
# Render-bound: big buffer, long strokes.
bench render --headless --time 10:08:30 --sim-seconds 12 --size 1600 --stroke-len 4
# Quantitative config: converged dt (4x the steps).
bench fine-dt --headless --time 10:08:30 --sim-seconds 8 --size 800 --particles 12000 --dt 0.008333
# Alternate face (tide arcs): field-heavy, many magnet elements.
bench tide --headless --face tide --time 10:08:30 --sim-seconds 12 --size 800
