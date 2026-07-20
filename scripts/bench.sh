#!/usr/bin/env bash
# Headless performance benchmark. Times the release binary on a few
# representative configs and reports, over RUNS runs, the MIN wall time (min
# is the stable estimator on a turbo-boosting CPU; see agent_docs/gotchas.md)
# plus an approx fps = fixed-dt sim frames (sim-seconds / dt) per wall-second.
# Whole-process wall (init + sim + one render + PNG encode), so both are a
# coarse regression signal, not a per-frame profile.
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
  # Frame count = simulated display time / dt (dt defaults to the sim's 1/30),
  # so fps below is fixed-dt sim frames per wall-second.
  local sim_s=0 dt=0.0333333333 prev=""
  for a in "$@"; do
    case "$prev" in
    --sim-seconds) sim_s=$a ;;
    --dt) dt=$a ;;
    esac
    prev=$a
  done
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
    awk -v n="$name" -v r="$RUNS" -v steps="$(awk "BEGIN{print $sim_s / $dt}")" \
      'NR==1||$1<m{m=$1} END{printf "  %-12s %7.3f s  %6.0f fps  (min of %d)\n", n, m, steps/m, r}'
}

echo "headless benchmark: $(nproc) cores, $RUNS runs each"
echo "  wall = min over runs (lower better); fps = fixed-dt sim frames/wall-second, incl. 1 render (higher better)"
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
