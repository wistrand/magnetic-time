#!/usr/bin/env bash
# Launch the clock in kiosk mode (fullscreen, dev panel hidden). Extra flags
# are passed through, e.g. bin/magnetic-time-kiosk.sh --pad 0.1 --face tide
set -euo pipefail
cd "$(dirname "$0")/.."
exec target/release/magnetic-time --kiosk --autosave --rotate 180 --disturb-every 3500 "$@"
