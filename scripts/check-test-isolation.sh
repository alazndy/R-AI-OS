#!/usr/bin/env bash
# Fails if the test suite touches the user's real raios config directory.
#
# Runs the whole workspace test suite with HOME/XDG_* pointed at an empty
# sandbox and then asserts no raios state (workspace.db, session token, ...)
# was created there. Tests that redirect HOME/XDG_CONFIG_HOME or
# RAIOS_DB_PATH themselves (raios_core::test_support::IsolatedDb) leave the
# sandbox untouched; a test that opens the default location does not.
#
# --test-threads=1 is deliberate: with parallel threads, a test that holds
# RAIOS_DB_PATH makes unguarded tests running beside it use that temp DB
# instead of the default one, which would hide the very leak we look for.
#
# Usage: scripts/check-test-isolation.sh [extra cargo test args...]
set -euo pipefail

cd "$(dirname "$0")/.."

sandbox="$(mktemp -d "${TMPDIR:-/tmp}/raios-isolation.XXXXXX")"
trap 'rm -rf "$sandbox"' EXIT

# Cargo and rustup need their real homes to find the toolchain and registry.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"

HOME="$sandbox" \
XDG_CONFIG_HOME="$sandbox/.config" \
XDG_DATA_HOME="$sandbox/.local/share" \
XDG_STATE_HOME="$sandbox/.local/state" \
XDG_CACHE_HOME="$sandbox/.cache" \
  cargo test --workspace --no-fail-fast "$@" -- --test-threads=1

# Ignore build/model caches that tooling may drop under XDG_CACHE_HOME.
leaked="$(find "$sandbox" -type f -not -path "$sandbox/.cache/*" | sed "s#^$sandbox/##" | sort)"

if [ -n "$leaked" ]; then
  echo "FAIL: tests wrote to the real config/data location:" >&2
  echo "$leaked" | sed 's/^/  /' >&2
  echo "Wrap the offending test in raios_core::test_support::IsolatedDb::new()." >&2
  exit 1
fi
echo "OK: the test suite left the user config directory untouched."
