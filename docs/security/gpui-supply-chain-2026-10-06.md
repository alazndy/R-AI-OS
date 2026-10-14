# GPUI supply-chain verification — 2026-10-06

Status: verification complete, owner decision **A (accept with controls)**, recorded 2026-10-06.

## What was checked

The native workspace plans to depend on GPUI through `gpui-pre` 0.3.8, `gpui-pre-platform` 0.3.8 and `gpui-base` 0.7.1 (the editor and components from `gpui-kit`). Resolving them gives 23 registry crates in the GPUI family (names starting with `gpui-pre`, `gpui-kit`, `gpui-base`, `gpui-component`); the full lockfile is `docs/security/gpui-0.3.8.Cargo.lock` (737 packages in total).

For each of the 23 crates `tools/raios-workspace-native/scripts/verify_gpui_supply_chain.py` checks:

1. the lockfile checksum equals the crates.io checksum and the SHA-256 of the downloaded `.crate` bytes;
2. the version is not yanked, and the license is reported;
3. the crate is owned only by the expected crates.io user (`huacnlee`);
4. the packaged source equals upstream source. The `gpui-pre*` crates are compared with Zed at commit `279fe07` (named in each crate's description, "gpui-pre snapshot of zed@279fe07"); `gpui-base` is compared with `longbridge/gpui-kit` at commit `87d10ae` (from its `.cargo_vcs_info.json`). Manifests, lockfiles and license files are ignored. Any other differing or added file must be in `gpui-supply-chain-allowlist.toml`, pinned by the SHA-256 of the reviewed file or as a verbatim upstream copy.

## Result

23 crates checked, 0 findings, exit status 0.

- All 23 are Apache-2.0, which is compatible with this repository's AGPL-3.0.
- All 23 have exactly one owner on crates.io: `huacnlee` (Jason Lee, maintainer of longbridge/gpui-component). None is owned by Zed Industries.
- All checksums match crates.io and the downloaded bytes; none is yanked.
- 19 crates are identical to upstream apart from their manifests.
- 4 crates differ from upstream in small, reviewed ways (next section).

| Crate | Version | License | Owner | Compared with | Differences |
|---|---|---|---|---|---|
| gpui-base | 0.7.1 | Apache-2.0 | huacnlee | longbridge/gpui-kit @ 87d10ae | none |
| gpui-pre | 0.3.8 | Apache-2.0 | huacnlee | zed @ 279fe07 | src/action.rs |
| gpui-pre-apple | 0.3.8 | Apache-2.0 | huacnlee | zed @ 279fe07 | build.rs, 5 vendored files |
| gpui-pre-macros | 0.3.8 | Apache-2.0 | huacnlee | zed @ 279fe07 | src/gpui_macros.rs, src/gpui_pre_facade_paths.rs |
| 19 other gpui-pre* crates | 0.3.8 | Apache-2.0 | huacnlee | zed @ 279fe07 | none |

The 19 identical crates are gpui-pre-collections, -derive-refineable, -http-client, -linux, -macos, -perf, -platform, -refineable, -scheduler, -shared-string, -sum-tree, -util, -util-macros, -web, -wgpu, -windows, -zlog, -ztracing and -ztracing-macro.

## Differences from upstream, reviewed by hand

1. **gpui-pre, src/action.rs.** The `actions!` macro derives `gpui::Action`; the published crate writes `$crate::Action` so the crate works under the name `gpui-pre`, and adds one comment line.
2. **gpui-pre-macros, src/gpui_macros.rs.** 0 lines removed, 79 added. Every proc-macro entry point is wrapped as `rewrite(__gpui_pre_<name>(...))` and the original function body is kept unchanged under the new name.
3. **gpui-pre-macros, src/gpui_pre_facade_paths.rs (new).** This is code that runs inside the compiler on the developer's machine. It rewrites `gpui::` and `gpui_platform::` paths in generated token streams to the facade crate, resolving its name with `proc_macro_crate`. It contains no process, network, filesystem or environment access (scanned for `std::process`, `Command::new`, `std::fs`, `std::net`, `reqwest`, `ureq`, `env::var`, `include_bytes!`, `unsafe`: 0 hits).
4. **gpui-pre-apple, build.rs.** One path changed from `../gpui` to `vendor/gpui` so the published crate is self-contained, plus one comment line. This crate is macOS-only and is not compiled on Linux. Its other process and file calls are upstream's own Metal shader build code.
5. **gpui-pre-apple, vendor/gpui/src/{color,geometry,platform,scene,window}.rs.** Verified byte-identical to Zed's `crates/gpui/src/` files at the same commit.

The allowlist pins items 1-4 by the SHA-256 of the reviewed published file and item 5 as verbatim copies, so a new release with different content produces a finding again and must be reviewed.

## What this does not establish

- The `gpui-pre*` crates have no `.cargo_vcs_info.json`, so their provenance rests on the commit named in the description plus the byte comparison above, not on a signature.
- Zed's own source and the `gpui-kit` repository were compared against, not audited.
- The other 714 packages in the lockfile are ordinary third-party crates; `cargo audit` covers known advisories for them, not this script.
- A single maintainer can publish a new version of all 23 crates. Protection against that is the exact pins (`=0.3.8`, `=0.7.1`), a committed lockfile and re-running this script on every GPUI bump.

## How to repeat

Run from the repository root:

    cargo generate-lockfile --manifest-path tools/raios-workspace-native/target/supply-chain/Cargo.toml
    python3 tools/raios-workspace-native/scripts/verify_gpui_supply_chain.py docs/security/gpui-0.3.8.Cargo.lock --expected-owner huacnlee --allowlist tools/raios-workspace-native/scripts/gpui-supply-chain-allowlist.toml

The decision logic has offline tests: `python3 -m unittest tools/raios-workspace-native/scripts/test_verify_gpui_supply_chain.py`.

## Decision

Status: **DECIDED by the owner on 2026-10-06: option A.** The agent that produced this report did not make this decision.

Options:

- **A. Accept with controls (recommended):** keep exact pins and the committed lockfile, run the script at every GPUI bump, and review any finding before upgrading.
- **B. Vendor the sources:** copy the 23 crates into the repository, as was done for `hf-hub` and `glib`. Immune to yanks and to a malicious new release, at the cost of repository size and manual upgrades. Choose this if the script ever reports a finding or a crate is yanked.
- **C. Do not depend on this family:** fall back to the egui approach from the spec.

Recorded decision: A. Accept with controls: exact pins, a committed lockfile, and this script run at every GPUI bump. Option B (vendoring) becomes mandatory if the script ever reports a finding or a crate is yanked.
