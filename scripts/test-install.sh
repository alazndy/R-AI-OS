#!/usr/bin/env bash
# Tests for the service handling in install.sh, using stub `systemctl` and
# `pgrep` so no real service, daemon or install is touched.
#
# Covers: which units get restarted (hub / legacy / transient / tray), ExecStart
# drift repair, detection of a daemon left running the old binary, and option
# parsing. Run: scripts/test-install.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(dirname "$here")"

work="$(mktemp -d "${TMPDIR:-/tmp}/raios-install-test.XXXXXX")"
pids=()
cleanup() {
  for p in "${pids[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
  rm -rf "$work"
}
trap cleanup EXIT

failures=0
pass() { echo "  ok   - $1"; }
fail() { echo "  FAIL - $1"; failures=$((failures + 1)); }
check() { # check "description" <command...>
  local desc="$1"; shift
  if "$@"; then pass "$desc"; else fail "$desc"; fi
}

# ─── stubs ───────────────────────────────────────────────────────────────────
stubs="$work/stubs"; mkdir -p "$stubs"
export STUB_LOG="$work/systemctl.log"

cat > "$stubs/systemctl" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$STUB_LOG"
if [ "$1 $2 $3 $4" = "--user show -p LoadState" ]; then
  unit="${*: -1}"
  for u in ${STUB_LOADED:-}; do
    if [ "$u" = "$unit" ]; then echo loaded; exit 0; fi
  done
  echo not-found
fi
exit 0
STUB

cat > "$stubs/pgrep" <<'STUB'
#!/usr/bin/env bash
# Only the processes the test started - never the developer's real aiosd.
for p in ${STUB_PIDS:-}; do echo "$p"; done
STUB
chmod +x "$stubs/systemctl" "$stubs/pgrep"
export PATH="$stubs:$PATH"

export HOME="$work/home"
mkdir -p "$HOME/.config/systemd/user"

# shellcheck source=../install.sh
source "$repo/install.sh"   # defines the helpers; does not run main
set +e                      # the tests inspect exit codes themselves

quiet() { "$@" >/dev/null 2>&1; }
restarted() { grep -c "^--user restart $1\$" "$STUB_LOG" || true; }
reset_log() { : > "$STUB_LOG"; }

# ─── which units are restarted ───────────────────────────────────────────────
echo "restart_units"
reset_log; STUB_LOADED="raios-hub.service raios-tray.service" restart_units >/dev/null
check "restarts raios-hub.service (what 'raios hub install' creates)" test "$(restarted raios-hub.service)" = 1
check "restarts the tray too" test "$(restarted raios-tray.service)" = 1
check "does not touch an aiosd.service that is not loaded" test "$(restarted aiosd.service)" = 0

reset_log; STUB_LOADED="aiosd.service" restart_units >/dev/null
check "still restarts a legacy/transient aiosd.service" test "$(restarted aiosd.service)" = 1
check "...and not a hub unit that does not exist" test "$(restarted raios-hub.service)" = 0

reset_log; STUB_LOADED="" restart_units >/dev/null
check "restarts nothing when no unit is loaded" test "$(grep -c -- '--user restart' "$STUB_LOG" || true)" = 0

# ─── ExecStart drift ─────────────────────────────────────────────────────────
echo "fix_exec_drift"
unit="$HOME/.config/systemd/user/raios-hub.service"
printf '[Service]\nExecStart=/old/place/aiosd\nRestart=on-failure\n' > "$unit"
reset_log; fix_exec_drift /new/bin >/dev/null
check "rewrites a drifted ExecStart" grep -qx 'ExecStart=/new/bin/aiosd' "$unit"
check "keeps the rest of the unit" grep -qx 'Restart=on-failure' "$unit"
check "reloads systemd after changing a unit" grep -qx -- '--user daemon-reload' "$STUB_LOG"
check "leaves no backup file behind" test ! -e "$unit.bak"

reset_log; fix_exec_drift /new/bin >/dev/null
check "an up-to-date unit is left alone (no daemon-reload)" test "$(grep -c daemon-reload "$STUB_LOG" || true)" = 0

rm -f "$unit"
printf '[Service]\nExecStart=/somewhere/aiosd\n' > "$HOME/.config/systemd/user/aiosd.service"
fix_exec_drift /new/bin >/dev/null
check "also repairs the legacy aiosd.service" grep -qx 'ExecStart=/new/bin/aiosd' "$HOME/.config/systemd/user/aiosd.service"

# ─── stale daemon detection ──────────────────────────────────────────────────
echo "check_running_daemons"
if [ -d /proc ]; then
  runbin="$work/run/aiosd"; mkdir -p "$work/run" "$work/new"
  cp "$(type -P sleep)" "$runbin"
  "$runbin" 60 & pid=$!; pids+=("$pid")
  export STUB_PIDS="$pid"

  cp "$runbin" "$work/new/aiosd"
  check "passes when the running daemon is the installed binary" quiet check_running_daemons "$work/new"

  cp "$(type -P true)" "$work/new/aiosd"
  out="$(check_running_daemons "$work/new" 2>&1)"; rc=$?
  check "fails when the installed binary differs from the running one" test "$rc" != 0
  check "...and says which pid is stale" grep -q "pid $pid" <<<"$out"
else
  echo "  skip - no /proc on this platform"
fi

# ─── option parsing ──────────────────────────────────────────────────────────
echo "option parsing"
out="$("$repo/install.sh" --help 2>&1)"; rc=$?
check "--help prints usage and exits 0" test "$rc" = 0 -a -n "$(grep -m1 Usage <<<"$out")"
"$repo/install.sh" --bogus >/dev/null 2>&1; rc=$?
check "an unknown option exits 2" test "$rc" = 2
"$repo/install.sh" --root >/dev/null 2>&1; rc=$?
check "--root without a value exits 2" test "$rc" = 2
check "install.sh is valid bash" bash -n "$repo/install.sh"

echo
if [ "$failures" -eq 0 ]; then echo "all install.sh checks passed"; else echo "$failures check(s) FAILED"; exit 1; fi
