#!/usr/bin/env bash
# Build and (re)install R-AI-OS on Linux/macOS, guaranteeing only one
# copy of raios/aiosd is ever active: `cargo install --force` replaces
# the binaries in place, and any running daemon/tray using the old
# binary is restarted - and then *checked* - so it can't keep serving
# stale code while the script claims success.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

# systemd user units that run the installed binaries.
#   raios-hub.service  - what `raios hub install` creates today
#   aiosd.service      - the older name, and what a hand-rolled or transient
#                        (`systemd-run --user`) daemon is usually called
#   raios-tray.service - the tray, which also runs installed code
# A unit counts as present when systemd has it loaded, which - unlike
# `list-unit-files` - also covers transient units that have no file on disk.
DAEMON_UNITS="raios-hub.service aiosd.service"
TRAY_UNITS="raios-tray.service"

USER_UNIT_DIR="$HOME/.config/systemd/user"

usage() {
  cat <<'EOF'
Usage: ./install.sh [--root <directory>] [--no-restart]

  --root <directory>  Install binaries under <directory>/bin instead of the active Cargo/PATH root.
  --no-restart        Do not restart user services; intended for CI and staged installs.
EOF
}

# True if systemd has the user unit loaded (file-backed or transient).
unit_loaded() {
  [ "$(systemctl --user show -p LoadState --value "$1" 2>/dev/null || true)" = "loaded" ]
}

# `raios hub install` bakes the aiosd path into ExecStart once, resolved via
# `which aiosd` at that moment, while this script resolves its own BIN_DIR
# separately via `command -v raios`. If PATH order differs between the two, a
# restart keeps serving a stale binary while looking like it worked, so rewrite
# any daemon unit whose ExecStart points somewhere else than this install.
fix_exec_drift() {
  local bin_dir="$1" desired="$1/aiosd" unit file current changed=0
  for unit in $DAEMON_UNITS; do
    file="$USER_UNIT_DIR/$unit"
    [ -f "$file" ] || continue
    current="$(grep -m1 '^ExecStart=' "$file" | cut -d= -f2- || true)"
    if [ -n "$current" ] && [ "$current" != "$desired" ]; then
      echo "[install] $unit ExecStart drift detected:"
      echo "[install]   unit points to : $current"
      echo "[install]   this install to: $desired"
      echo "[install] rewriting ExecStart to match this install..."
      sed -i.bak "s|^ExecStart=.*|ExecStart=${desired}|" "$file" && rm -f "$file.bak"
      changed=1
    fi
  done
  if [ "$changed" -eq 1 ]; then
    systemctl --user daemon-reload
  fi
}

restart_units() {
  local unit
  for unit in $DAEMON_UNITS $TRAY_UNITS; do
    if unit_loaded "$unit"; then
      echo "[install] restarting $unit so it picks up the new binary..."
      systemctl --user restart "$unit" || echo "[install] could not restart $unit (may not be running)"
    fi
  done
}

# Succeeds only if every running aiosd executes exactly the installed binary.
# A daemon that was not restarted (started by hand, or under a unit this script
# does not know about) is reported instead of being left quietly stale.
# Linux only: needs /proc/<pid>/exe, which also works for a replaced binary.
check_running_daemons() {
  local bin_dir="$1" pid stale=0
  [ -d /proc ] || return 0
  command -v pgrep >/dev/null 2>&1 || return 0
  for pid in $(pgrep -x aiosd 2>/dev/null || true); do
    # Another user's process cannot be read; nothing to compare.
    [ -r "/proc/$pid/exe" ] || continue
    if ! cmp -s "/proc/$pid/exe" "$bin_dir/aiosd"; then
      echo "[install] WARNING: aiosd (pid $pid) is still running the OLD binary."
      echo "[install]          It was not restarted. Restart it, e.g.: systemctl --user restart raios-hub.service"
      echo "[install]          (or stop it and start $bin_dir/aiosd yourself)."
      stale=1
    fi
  done
  [ "$stale" -eq 0 ]
}

main() {
  local install_root_override="" restart_services=1

  while [ "$#" -gt 0 ]; do
    case "$1" in
      --root)
        [ "$#" -ge 2 ] || { echo "[install] --root requires a directory" >&2; exit 2; }
        install_root_override="$2"
        shift 2
        ;;
      --no-restart)
        restart_services=0
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

  local bin_dir install_root
  if [ -n "$install_root_override" ]; then
    install_root="$install_root_override"
    bin_dir="$install_root/bin"
  else
    local default_bin_dir active_raios
    default_bin_dir="$(dirname "$(command -v cargo)")"
    active_raios="$(command -v raios 2>/dev/null || true)"
    if [ -n "$active_raios" ]; then
      bin_dir="$(dirname "$active_raios")"
    else
      bin_dir="$default_bin_dir"
    fi
    install_root="$(dirname "$bin_dir")"
  fi

  # One release build. `cargo install` normally compiles in a throwaway
  # directory, which meant building the whole tree a second time; pointing it
  # at the workspace target dir reuses (and warms) the normal release build.
  # It still builds before replacing anything, so a failed build leaves the
  # existing install untouched.
  local target_dir="${CARGO_TARGET_DIR:-$PWD/target}"
  echo "[install] building and installing raios + aiosd into ${bin_dir} (replacing any existing install)..."
  cargo install --path crates/raios-surface-cli --locked --force \
    --root "$install_root" --target-dir "$target_dir"

  echo "[install] checking for stray raios/aiosd binaries outside ${bin_dir}..."
  local dir name
  local -a PATH_DIRS
  IFS=':' read -ra PATH_DIRS <<< "$PATH"
  for dir in "${PATH_DIRS[@]}"; do
    [ "$dir" = "$bin_dir" ] && continue
    for name in raios aiosd; do
      if [ -x "$dir/$name" ]; then
        echo "[install] WARNING: stray $name found at $dir/$name — this will shadow the freshly installed version on PATH."
      fi
    done
  done

  local exit_code=0
  if [ "$restart_services" -eq 1 ] && command -v systemctl >/dev/null 2>&1; then
    fix_exec_drift "$bin_dir"
    restart_units
    # Give a restarted daemon a moment to be the one pgrep sees.
    sleep 1
    check_running_daemons "$bin_dir" || exit_code=1
  fi

  echo "[install] done — active binaries:"
  printf '%s\n' "$bin_dir/raios"
  printf '%s\n' "$bin_dir/aiosd"
  "$bin_dir/raios" --version
  return "$exit_code"
}

# Run only when executed, so the helper functions can be sourced by tests.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
