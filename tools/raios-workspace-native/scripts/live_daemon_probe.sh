#!/bin/bash
# Runs the native workspace's DaemonClient against a REAL aiosd built from this repository, in an
# isolated user+network namespace with a temporary config directory and database, so the user's
# daemon, ports, session token and workspace.db are never touched. The probe makes real
# mutating calls (register, handoff, launch validation) against that throwaway daemon.
#
# Needs (set DAEMON_CWD to a checkout that has the .fastembed_cache): `cargo build -p raios-surface-cli --bin aiosd` in the repository root, the fastembed
# model cache in the repository root (the daemon loads it at start), and `unshare` with user
# namespaces enabled.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
NATIVE="$ROOT/tools/raios-workspace-native"
AIOSD="${AIOSD:-${CARGO_TARGET_DIR_ROOT:-$HOME/.cache/raios-root-target}/debug/aiosd}"
PROBE="${PROBE:-${CARGO_TARGET_DIR:-$HOME/.cache/rwk-native-target}/debug/examples/live_probe}"
WORK="$(mktemp -d)"
mkdir -p "$WORK/config" "$WORK/proj"
cat > "$WORK/inner.sh" <<INNER
#!/bin/bash
ip link set lo up 2>/dev/null
export XDG_CONFIG_HOME="$WORK/config" RAIOS_DB_PATH="$WORK/workspace.db"
cd "${DAEMON_CWD:-$ROOT}"
"$AIOSD" > "$WORK/aiosd.log" 2>&1 &
DAEMON=\$!
for _ in \$(seq 1 90); do (echo > /dev/tcp/127.0.0.1/42071) 2>/dev/null && break; sleep 1; done
sleep 3
"$PROBE" "$WORK/proj"
kill \$DAEMON 2>/dev/null || true
INNER
chmod +x "$WORK/inner.sh"
(cd "$NATIVE" && cargo build -q -p rwk-app --example live_probe)
timeout 280 unshare -rn "$WORK/inner.sh"
echo "daemon log: $WORK/aiosd.log"
