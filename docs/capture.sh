#!/usr/bin/env bash
# Regenerate the PNG screenshots used by docs/index.html.
#
# See docs/README.md for the full workflow (add a shot, update copy, etc.).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PLAYWRIGHT_NODE_MODULES="${PLAYWRIGHT_NODE_MODULES:-$HOME/.local/share/mise/installs/npm-playwright/latest/node_modules}"
if [ ! -d "$PLAYWRIGHT_NODE_MODULES/playwright" ]; then
  PLAYWRIGHT_NODE_MODULES="$(node -e 'console.log(require("path").dirname(require.resolve("playwright/package.json")))' 2>/dev/null || true)"
  if [ -n "${PLAYWRIGHT_NODE_MODULES:-}" ]; then
    PLAYWRIGHT_NODE_MODULES="$(dirname "$PLAYWRIGHT_NODE_MODULES")"
  fi
fi

if [ ! -d "${PLAYWRIGHT_NODE_MODULES:-/nonexistent}/playwright" ]; then
  printf 'error: cannot find the playwright Node package.\n' >&2
  printf 'Install it (npm i -g playwright) or set PLAYWRIGHT_NODE_MODULES.\n' >&2
  exit 1
fi

info() { printf '==> %s\n' "$*"; }

info "render TUI frames"
cargo run -p dd_ftp_cli --example tutorial_shots --release

info "screenshot frames to PNG"
NODE_PATH="$PLAYWRIGHT_NODE_MODULES${NODE_PATH:+:$NODE_PATH}" \
  node "$ROOT/docs/rasterize.mjs"

info "check docs/index.html references"
missing=0
while IFS= read -r img; do
  path="$ROOT/docs/$img"
  if [ ! -f "$path" ]; then
    printf 'missing image referenced by docs/index.html: %s\n' "$img" >&2
    missing=1
  fi
done < <(grep -oE 'images/[0-9][-a-z0-9]+\.png' "$ROOT/docs/index.html" | sort -u)

if [ "$missing" -ne 0 ]; then
  exit 1
fi

info "done. PNGs are in docs/images/"
