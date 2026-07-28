#!/usr/bin/env bash
# Delete the autosave config so the next start uses defaults/flags only.
# Keeps the previous config as autosave.json.bak next to it.
set -euo pipefail
f="$HOME/.config/magnetic-time/autosave.json"
if [ -e "$f" ]; then
    mv "$f" "$f.bak"
    echo "moved $f -> $f.bak"
else
    echo "no autosave file at $f"
fi
