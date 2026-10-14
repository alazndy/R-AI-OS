#!/usr/bin/env python3
"""Verify the GPUI registry crates a Cargo.lock pins against crates.io and their upstream source.

For every selected crate (default prefixes: gpui-pre, gpui-kit, gpui-base, gpui-component) it
checks that
  1. the lockfile checksum equals the crates.io checksum and the SHA-256 of the downloaded
     .crate bytes,
  2. the version is not yanked and its license is reported,
  3. the crate has the expected owners (--expected-owner), and
  4. the packaged source equals upstream source: gpui-pre* against Zed at the commit named in
     the crate description ("snapshot of zed@<sha>"), others against the git commit in
     .cargo_vcs_info.json. A file that differs must be in the allowlist, pinned either by the
     SHA-256 of the reviewed published file or as a verbatim copy of an upstream file.

Exit status: 0 clean, 1 findings, 2 network or input error. Standard library only.
"""

import argparse
import hashlib
import io
import json
import re
import sys
import tarfile
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

USER_AGENT = {"User-Agent": "raios-workspace-supply-chain-check (github.com/alazndy/R-AI-OS)"}
DEFAULT_PREFIXES = ("gpui-pre", "gpui-kit", "gpui-base", "gpui-component")
IGNORED_NAMES = {"Cargo.toml", "Cargo.lock", "Cargo.toml.orig", ".cargo_vcs_info.json", ".cargo-ok"}
MAX_UPSTREAM_FILE_BYTES = 8 * 1024 * 1024
API_DELAY_SECONDS = 1.1
ZED_REPO = "zed-industries/zed"


class NetworkError(Exception):
    """Raised when a required download fails."""


def fetch(url, stream=False):
    request = urllib.request.Request(url, headers=USER_AGENT)
    try:
        response = urllib.request.urlopen(request, timeout=120)
    except (urllib.error.URLError, TimeoutError) as error:
        raise NetworkError(f"{url}: {error}") from error
    return response if stream else response.read()


def crates_api(path):
    time.sleep(API_DELAY_SECONDS)
    return json.loads(fetch(f"https://crates.io/api/v1/{path}"))


def is_ignored(relative):
    name = relative.rsplit("/", 1)[-1]
    return name in IGNORED_NAMES or name.startswith("LICENSE")


def read_lock(path, prefixes):
    lock = tomllib.loads(Path(path).read_text())
    selected = []
    for package in lock.get("package", []):
        source = package.get("source", "")
        if source.startswith("registry+") and package["name"].startswith(prefixes):
            selected.append((package["name"], package["version"], package.get("checksum", "")))
    return sorted(selected)


def crate_members(crate_bytes, name, version):
    """Yields (relative_path, bytes) for every regular file of a .crate archive."""
    prefix = f"{name}-{version}/"
    with tarfile.open(fileobj=io.BytesIO(crate_bytes), mode="r:gz") as archive:
        for member in archive:
            if member.isfile() and member.name.startswith(prefix):
                yield member.name[len(prefix):], archive.extractfile(member).read()


def crate_files(crate_bytes, name, version):
    return {p: b for p, b in crate_members(crate_bytes, name, version) if not is_ignored(p)}


def crate_member(crate_bytes, name, version, relative):
    return dict(crate_members(crate_bytes, name, version)).get(relative)


def candidate_zed_dirs(name):
    """Directories that may hold the upstream source of a gpui-pre* crate."""
    rest = name[len("gpui-pre"):].lstrip("-").replace("-", "_")
    if not rest:
        return ["crates/gpui"]
    return [f"crates/gpui_{rest}", f"crates/{rest}", f"crates/refineable/{rest}", f"tooling/{rest}"]


def download_upstream(repo, sha, wanted_dirs):
    """Streams one commit tarball and returns {dir: {relative_path: bytes}} for wanted_dirs."""
    found = {directory: {} for directory in wanted_dirs}
    response = fetch(f"https://codeload.github.com/{repo}/tar.gz/{sha}", stream=True)
    with tarfile.open(fileobj=response, mode="r|gz") as archive:
        for member in archive:
            if not member.isfile() or member.size > MAX_UPSTREAM_FILE_BYTES:
                continue
            parts = member.name.split("/", 1)
            inner = parts[1] if len(parts) == 2 else ""
            matches = [d for d in wanted_dirs if inner.startswith(d + "/")]
            if not matches:
                continue
            # A stream cannot be rewound, and wanted directories may be nested, so read once.
            data = archive.extractfile(member).read()
            for directory in matches:
                relative = inner[len(directory) + 1:]
                if not is_ignored(relative):
                    found[directory][relative] = data
    return found


def compare(published, upstream):
    changed = sorted(p for p in published if p in upstream and published[p] != upstream[p])
    published_only = sorted(p for p in published if p not in upstream)
    return changed, published_only


def load_allowlist(path):
    """Returns {(crate, path): entry}. An entry is pinned either by the SHA-256 of the published
    file (`sha256`) or by being a byte-identical copy of an upstream file (`copy_of`)."""
    if not path or not Path(path).exists():
        return {}
    entries = tomllib.loads(Path(path).read_text()).get("allow", [])
    for entry in entries:
        if ("sha256" in entry) == ("copy_of" in entry):
            raise SystemExit(f"allowlist entry needs exactly one of sha256/copy_of: {entry}")
    return {(entry["crate"], entry["path"]): entry for entry in entries}


def copy_source_dir(copy_of):
    """`crates/gpui/src/color.rs` -> ('crates/gpui', 'src/color.rs')."""
    parts = copy_of.split("/")
    return "/".join(parts[:2]), "/".join(parts[2:])


def allowlist_verdict(entry, version, relative, published, upstream_by_dir):
    """Returns (allowed, reason_or_problem)."""
    if entry is None:
        return False, None
    if entry.get("version", version) != version:
        return False, "allowlist entry is for another version"
    if "sha256" in entry:
        if hashlib.sha256(published[relative]).hexdigest() != entry["sha256"]:
            return False, "content differs from the reviewed hash"
        return True, entry["reason"]
    directory, inner = copy_source_dir(entry["copy_of"])
    if upstream_by_dir.get(directory, {}).get(inner) != published[relative]:
        return False, f"is not a verbatim copy of upstream {entry['copy_of']}"
    return True, entry["reason"]


def check_registry(name, version, lock_checksum):
    findings = []
    meta = crates_api(f"crates/{name}/{version}")["version"]
    crate_bytes = fetch(f"https://static.crates.io/crates/{name}/{name}-{version}.crate")
    actual = hashlib.sha256(crate_bytes).hexdigest()
    if lock_checksum != meta["checksum"]:
        findings.append("lockfile checksum differs from crates.io")
    if actual != meta["checksum"]:
        findings.append("downloaded bytes differ from the crates.io checksum")
    if meta.get("yanked"):
        findings.append("version is yanked")
    return crate_bytes, meta, findings


def check_owners(name, expected):
    users = crates_api(f"crates/{name}/owner_user").get("users", [])
    teams = crates_api(f"crates/{name}/owner_team").get("teams", [])
    owners = sorted([u["login"] for u in users] + [t["login"] for t in teams])
    return owners, [o for o in owners if expected and o not in expected]


def resolve_upstream(name, version, crate_bytes):
    """Returns (repo, sha, candidate_dirs) or None when no upstream reference is available."""
    manifest = (crate_member(crate_bytes, name, version, "Cargo.toml") or b"").decode()
    if name.startswith("gpui-pre"):
        match = re.search(r"zed@([0-9a-f]{7,40})", manifest)
        return (ZED_REPO, match.group(1), candidate_zed_dirs(name)) if match else None
    raw = crate_member(crate_bytes, name, version, ".cargo_vcs_info.json")
    repo = re.search(r'repository\s*=\s*"https://github.com/([^"]+?)(?:\.git)?"', manifest)
    if not raw or not repo:
        return None
    info = json.loads(raw)
    return repo.group(1), info["git"]["sha1"], [info.get("path_in_vcs", "")]


def inspect_crates(crates, expected_owners, allowlist):
    """Registry, owner and upstream-reference checks plus the upstream downloads."""
    rows, wanted = [], {}
    for name, version, lock_checksum in crates:
        crate_bytes, meta, findings = check_registry(name, version, lock_checksum)
        owners, unexpected = check_owners(name, expected_owners)
        if unexpected:
            findings.append(f"unexpected owners: {', '.join(unexpected)}")
        reference = resolve_upstream(name, version, crate_bytes)
        rows.append({
            "name": name, "version": version, "license": meta.get("license"), "owners": owners,
            "reference": reference, "findings": findings,
            "published": crate_files(crate_bytes, name, version),
        })
        if reference:
            dirs = wanted.setdefault((reference[0], reference[1]), set())
            dirs.update(reference[2])
            dirs.update(copy_source_dir(e["copy_of"])[0] for (c, _), e in allowlist.items()
                        if c == name and "copy_of" in e)
    upstream = {key: download_upstream(key[0], key[1], sorted(dirs)) for key, dirs in wanted.items()}
    return rows, upstream


def evaluate(row, upstream, allowlist):
    """Compares one crate with upstream; returns (upstream_note, differing_file_descriptions)."""
    reference = row["reference"]
    if not reference:
        row["findings"].append("no upstream reference to compare against")
        return "no upstream reference", []
    repo, sha, directories = reference
    by_dir = upstream[(repo, sha)]
    tree = next((by_dir[d] for d in directories if by_dir.get(d)), None)
    note = f"{repo}@{sha[:7]}"
    if tree is None:
        row["findings"].append("upstream directory not found")
        return f"{note} (directory not found)", []
    changed, added = compare(row["published"], tree)
    differing = []
    for relative in changed + added:
        kind = "changed" if relative in changed else "added"
        entry = allowlist.get((row["name"], relative))
        allowed, why = allowlist_verdict(entry, row["version"], relative, row["published"], by_dir)
        if allowed:
            differing.append(f"{relative} ({kind}; reviewed: {why})")
        else:
            differing.append(f"{relative} ({kind})")
            row["findings"].append(f"{why or 'unexplained'} {kind} file: {relative}")
    return note, differing


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("lockfile")
    parser.add_argument("--prefix", action="append", dest="prefixes")
    parser.add_argument("--expected-owner", action="append", default=[])
    parser.add_argument("--allowlist")
    args = parser.parse_args()

    allowlist = load_allowlist(args.allowlist)
    crates = read_lock(args.lockfile, tuple(args.prefixes or DEFAULT_PREFIXES))
    if not crates:
        print("no matching registry crates in the lockfile", file=sys.stderr)
        return 2
    try:
        rows, upstream = inspect_crates(crates, set(args.expected_owner), allowlist)
    except NetworkError as error:
        print(f"network error: {error}", file=sys.stderr)
        return 2

    print("| crate | version | license | owners | upstream | differing files |")
    print("|---|---|---|---|---|---|")
    for row in rows:
        note, differing = evaluate(row, upstream, allowlist)
        print(f"| {row['name']} | {row['version']} | {row['license']} | {', '.join(row['owners'])} "
              f"| {note} | {'; '.join(differing) or 'none'} |")

    print()
    for row in rows:
        for finding in row["findings"]:
            print(f"FINDING {row['name']} {row['version']}: {finding}")
    failures = sum(bool(row["findings"]) for row in rows)
    print(f"\n{len(rows)} crates checked, {failures} with findings")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
