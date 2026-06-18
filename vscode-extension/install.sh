#!/usr/bin/env bash
# Build and install the R-AI-OS VS Code extension, replacing any
# previously installed version so only one copy is ever present.
set -euo pipefail

cd "$(dirname "$0")"

EXT_ID="alazndy.raios"
VERSION=$(node -p "require('./package.json').version")
VSIX="raios-${VERSION}.vsix"

echo "[install] compiling..."
npm run compile

echo "[install] packaging ${VSIX}..."
npx vsce package

if code --list-extensions | grep -qx "$EXT_ID"; then
  echo "[install] removing existing ${EXT_ID} install..."
  code --uninstall-extension "$EXT_ID"
fi

echo "[install] installing ${VSIX}..."
code --install-extension "$VSIX" --force

echo "[install] done — installed version:"
code --list-extensions --show-versions | grep -i raios
