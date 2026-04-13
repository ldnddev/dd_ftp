#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
  printf "Error: cargo is required but not found in PATH.\n" >&2
  exit 1
fi

printf "Building dd_ftp (release)...\n"
cargo build --release -p dd_ftp_cli

BIN_SRC="$SCRIPT_DIR/target/release/dd_ftp_cli"
if [[ ! -x "$BIN_SRC" ]]; then
  printf "Error: built binary not found at %s\n" "$BIN_SRC" >&2
  exit 1
fi

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BIN_DST_NAME="${BIN_NAME:-dd_ftp}"
BIN_DST="$INSTALL_DIR/$BIN_DST_NAME"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$BIN_SRC" "$BIN_DST"

printf "Installed %s\n" "$BIN_DST"

if command -v "$BIN_DST_NAME" >/dev/null 2>&1; then
  printf "Run: %s --help\n" "$BIN_DST_NAME"
else
  printf "Note: %s is not currently in PATH. Add this to your shell config:\n" "$INSTALL_DIR"
  printf "  export PATH=\"%s:$PATH\"\n" "$INSTALL_DIR"
fi
