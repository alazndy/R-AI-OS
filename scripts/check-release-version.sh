#!/usr/bin/env bash
set -euo pipefail

release_tag="${1:-}"
if [[ ! "$release_tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "release tag must be strict semver with a v prefix (for example v3.9.0)" >&2
  exit 1
fi
expected_version="${release_tag#v}"

metadata="$(cargo metadata --locked --format-version 1)"
mapfile -t workspace_versions < <(
  jq -r '
    .workspace_members[] as $id
    | .packages[]
    | select(.id == $id)
    | .version
  ' <<<"$metadata" | sort -u
)

if [[ "${#workspace_versions[@]}" -ne 1 || "${workspace_versions[0]}" != "$expected_version" ]]; then
  echo "workspace crate versions do not all match $release_tag: ${workspace_versions[*]:-none}" >&2
  exit 1
fi

if ! grep -Fq "## $release_tag —" CHANGELOG.md; then
  echo "CHANGELOG.md has no dated $release_tag section; Unreleased must be finalized first" >&2
  exit 1
fi

if ! grep -Fq "version-$release_tag-blue" README.md; then
  echo "README.md core version badge does not match $release_tag" >&2
  exit 1
fi

if ! grep -Fq "RAIOS_VERSION=$release_tag" README.md; then
  echo "README.md pinned installer example does not match $release_tag" >&2
  exit 1
fi

extension_version="$(jq -r '.version' vscode-extension/package.json)"
if [[ ! "$extension_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "VS Code extension version is not strict semver: $extension_version" >&2
  exit 1
fi

if ! grep -Fq "VS%20Code-v$extension_version-" README.md; then
  echo "README.md VS Code badge does not match v$extension_version" >&2
  exit 1
fi

if ! grep -Fq "raios-$extension_version.vsix" README.md; then
  echo "README.md VSIX install filename does not match v$extension_version" >&2
  exit 1
fi

echo "release version preflight passed: $release_tag, VSIX v$extension_version"
