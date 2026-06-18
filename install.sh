#!/usr/bin/env bash
# Build and (re)install R-AI-OS on Linux/macOS, guaranteeing only one
# copy of raios/aiosd is ever active: `cargo install --force` replaces
# the binary in place, and any running daemon/tray using the old
# binary is restarted so it can't keep serving stale code.
set -euo pipefail

cd "$(dirname "$0")"

echo "[install] building release binaries..."
cargo build --release

echo "[install] installing raios + aiosd (replacing any existing install)..."
cargo install --path . --force

BIN_DIR="$(dirname "$(command -v cargo)")"
echo "[install] checking for stray raios/aiosd binaries outside ${BIN_DIR}..."
IFS=':' read -ra PATH_DIRS <<< "$PATH"
for dir in "${PATH_DIRS[@]}"; do
  [ "$dir" = "$BIN_DIR" ] && continue
  for name in raios aiosd; do
    if [ -x "$dir/$name" ]; then
      echo "[install] WARNING: stray $name found at $dir/$name — this will shadow the freshly installed version on PATH."
    fi
  done
done

if command -v systemctl >/dev/null 2>&1; then
  for unit in aiosd.service raios-tray.service; do
    if systemctl --user list-unit-files "$unit" 2>/dev/null | grep -q "$unit"; then
      echo "[install] restarting $unit so it picks up the new binary..."
      systemctl --user restart "$unit" || echo "[install] could not restart $unit (may not be running)"
    fi
  done
fi

echo "[install] done — active binaries:"
command -v raios
command -v aiosd
raios --version 2>/dev/null || true
