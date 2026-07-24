#!/usr/bin/env bash
# Build and (re)install R-AI-OS on Linux/macOS, guaranteeing only one
# copy of raios/aiosd is ever active: `cargo install --force` replaces
# the binary in place, and any running daemon/tray using the old
# binary is restarted so it can't keep serving stale code.
set -euo pipefail

cd "$(dirname "$0")"

INSTALL_ROOT_OVERRIDE=""
RESTART_SERVICES=1

usage() {
  cat <<'EOF'
Usage: ./install.sh [--root <directory>] [--no-restart]

  --root <directory>  Install binaries under <directory>/bin instead of the active Cargo/PATH root.
  --no-restart        Do not restart user services; intended for CI and staged installs.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --root)
      [ "$#" -ge 2 ] || { echo "[install] --root requires a directory" >&2; exit 2; }
      INSTALL_ROOT_OVERRIDE="$2"
      shift 2
      ;;
    --no-restart)
      RESTART_SERVICES=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "[install] unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

echo "[install] building release binaries..."
cargo build --release --workspace --locked

if [ -n "$INSTALL_ROOT_OVERRIDE" ]; then
  INSTALL_ROOT="$INSTALL_ROOT_OVERRIDE"
  BIN_DIR="$INSTALL_ROOT/bin"
else
  DEFAULT_BIN_DIR="$(dirname "$(command -v cargo)")"
  ACTIVE_RAIOS="$(command -v raios 2>/dev/null || true)"
  if [ -n "$ACTIVE_RAIOS" ]; then
    BIN_DIR="$(dirname "$ACTIVE_RAIOS")"
  else
    BIN_DIR="$DEFAULT_BIN_DIR"
  fi
  INSTALL_ROOT="$(dirname "$BIN_DIR")"
fi

echo "[install] installing raios + aiosd into ${BIN_DIR} (replacing any existing install)..."
cargo install --path crates/raios-surface-cli --locked --force --root "$INSTALL_ROOT"

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

if [ "$RESTART_SERVICES" -eq 1 ] && command -v systemctl >/dev/null 2>&1; then
  # aiosd.service's ExecStart is a path baked in once by `raios hub install`
  # (resolved via `which aiosd` at whatever moment that ran). This script's
  # own BIN_DIR is resolved independently via `command -v raios`, at install
  # time. Both are PATH-order-dependent, and they run at different moments —
  # if PATH order ever differs between those two moments, ExecStart silently
  # points at a directory this script isn't installing into, and a restart
  # keeps serving a stale binary while looking like it worked. Detect and
  # self-heal that drift before restarting, so "only one copy is ever
  # active" (see header) is actually guaranteed, not just assumed.
  AIOSD_UNIT="$HOME/.config/systemd/user/aiosd.service"
  if [ -f "$AIOSD_UNIT" ]; then
    CURRENT_EXEC="$(grep -m1 '^ExecStart=' "$AIOSD_UNIT" | cut -d= -f2-)"
    DESIRED_EXEC="${BIN_DIR}/aiosd"
    if [ -n "$CURRENT_EXEC" ] && [ "$CURRENT_EXEC" != "$DESIRED_EXEC" ]; then
      echo "[install] aiosd.service ExecStart drift detected:"
      echo "[install]   unit points to : $CURRENT_EXEC"
      echo "[install]   this install to: $DESIRED_EXEC"
      echo "[install] rewriting ExecStart to match this install..."
      sed -i "s|^ExecStart=.*|ExecStart=${DESIRED_EXEC}|" "$AIOSD_UNIT"
      systemctl --user daemon-reload
    fi
  fi

  for unit in aiosd.service raios-tray.service; do
    if systemctl --user list-unit-files "$unit" 2>/dev/null | grep -q "$unit"; then
      echo "[install] restarting $unit so it picks up the new binary..."
      systemctl --user restart "$unit" || echo "[install] could not restart $unit (may not be running)"
    fi
  done
fi

echo "[install] done — active binaries:"
printf '%s\n' "$BIN_DIR/raios"
printf '%s\n' "$BIN_DIR/aiosd"
"$BIN_DIR/raios" --version
