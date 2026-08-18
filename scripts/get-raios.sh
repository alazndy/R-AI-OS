#!/bin/sh
# Downloads and installs the latest (or a pinned) raios release binary for
# Linux or macOS on x86_64 — the only architecture the release workflow
# currently builds. Verifies the published sha256 checksum before installing.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/alazndy/R-AI-OS/master/scripts/get-raios.sh | sh
#
# Env overrides:
#   RAIOS_VERSION      Release tag to install, e.g. "v3.9.0" (default: latest)
#   RAIOS_INSTALL_DIR  Directory to install into (default: "$HOME/.local/bin")
#
# This only installs prebuilt release binaries. To build from source instead,
# use ./install.sh in a checkout of the repository.

set -eu

REPO="alazndy/R-AI-OS"
INSTALL_DIR="${RAIOS_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${RAIOS_VERSION:-}"

log() { printf '[get-raios] %s\n' "$1"; }
die() {
  printf '[get-raios] error: %s\n' "$1" >&2
  exit 1
}

# ── Detect platform ─────────────────────────────────────────────────────────

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux) target="linux-x86_64" ;;
  Darwin) target="macos-x86_64" ;;
  *) die "unsupported OS '$os' — releases only cover Linux and macOS. Windows: see the .zip asset at https://github.com/$REPO/releases, or build from source." ;;
esac

case "$arch" in
  x86_64 | amd64) ;;
  *) die "unsupported architecture '$arch' — releases only ship x86_64 binaries today (no arm64/Apple Silicon build yet). Build from source instead: git clone https://github.com/$REPO && cd R-AI-OS && ./install.sh" ;;
esac

# ── Resolve version ──────────────────────────────────────────────────────────

if [ -z "$VERSION" ]; then
  log "resolving latest release..."
  VERSION="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | head -1 | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
  [ -n "$VERSION" ] || die "could not resolve the latest release tag from the GitHub API"
fi
log "installing raios $VERSION for $target"

# ── Download + verify ────────────────────────────────────────────────────────

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

archive="raios-$target.tar.gz"
base_url="https://github.com/$REPO/releases/download/$VERSION"

log "downloading $archive..."
curl -fsSL -o "$tmp_dir/$archive" "$base_url/$archive" || die "failed to download $base_url/$archive — does release $VERSION exist?"
curl -fsSL -o "$tmp_dir/$archive.sha256" "$base_url/$archive.sha256" || die "failed to download the checksum file for $archive"

log "verifying checksum..."
(
  cd "$tmp_dir"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$archive.sha256"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$archive.sha256"
  else
    die "neither sha256sum nor shasum is available — cannot verify the download"
  fi
) || die "checksum verification failed — the downloaded archive does not match its published sha256"

# ── Install ───────────────────────────────────────────────────────────────────

log "extracting..."
tar -xzf "$tmp_dir/$archive" -C "$tmp_dir"
stage_dir="$tmp_dir/raios-$target"
[ -x "$stage_dir/raios" ] || die "extracted archive did not contain a 'raios' binary — unexpected archive layout"

mkdir -p "$INSTALL_DIR"
install -m 755 "$stage_dir/raios" "$INSTALL_DIR/raios"
if [ -f "$stage_dir/aiosd" ]; then
  install -m 755 "$stage_dir/aiosd" "$INSTALL_DIR/aiosd"
fi

log "installed to $INSTALL_DIR"
"$INSTALL_DIR/raios" --version || true

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    log "note: $INSTALL_DIR is not on your PATH. Add this to your shell profile:"
    log "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

log "done — run 'raios --help' to get started"
