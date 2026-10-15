# Task 1 — Native workspace CI

Status: all requested native CI gates passed on Ubuntu and Arch Linux. Latest run 38180647785 completed both native jobs successfully after the verified Arch font fix. The separate root-workspace Windows/macOS/security checks remain failures outside these native jobs.

## Changes

In `.worktrees/workspace-native-p3/.github/workflows/ci.yml`, added `native-workspace` on ubuntu-latest, with pinned existing checkout/toolchain/cache actions, Rust 1.96.0, CARGO_BUILD_JOBS=2, and target caching under `/home/runner/.cache/rwk-native-target`. It checks fmt, the rwk-core normal dependency boundary, GPUI provenance, clippy with warnings denied, headless tests, and the native lockfile audit. Test git configuration is isolated. The test step explicitly unsets RWK_UI_DISPLAY_TESTS: the existing window harness checks whether it exists, so setting it to 0 would incorrectly enable display tests. The harness was preserved.

Added `tools/raios-workspace-native/scripts/verify_core_boundary.py`. It obtains the locked `cargo tree -p rwk-core -e normal --prefix none`, rejects gpui*, raios-core, raios-runtime, raios-surface-*, tauri, wgpu and tokio, and fails closed on cargo failure. Positive and negative fixture checks passed.

## Commands and results

From the native worktree:

```sh
cargo fmt --manifest-path tools/raios-workspace-native/Cargo.toml --all --check
python3 tools/raios-workspace-native/scripts/verify_core_boundary.py tools/raios-workspace-native/Cargo.toml
python3 tools/raios-workspace-native/scripts/verify_gpui_supply_chain.py tools/raios-workspace-native/Cargo.lock --expected-owner huacnlee --allowlist tools/raios-workspace-native/scripts/gpui-supply-chain-allowlist.toml
```

All passed. Supply-chain verification checked 23 crates, zero findings. YAML parsing and native job/display-test guard checks passed. Native strict clippy and all tests passed as recorded in task 2. From the main checkout:

```sh
cargo audit --db .cache/rustsec-db --file .worktrees/workspace-native-p3/tools/raios-workspace-native/Cargo.lock
```

Audit exited zero: no vulnerabilities, five known unmaintained warnings (bincode, instant, paste, rustybuzz, ttf-parser). Advisory DB was fetched into a writable workspace cache. Cargo could not refresh the shared crates.io index due the sandbox's read-only package-cache lock; this warning was retained in `.tmp/native-audit.log`. No ignore rule or audit gate was weakened.

## Remote evidence and unresolved step

Read-only `gh run list --repo alazndy/R-AI-OS --branch feat/workspace-native --limit 3 --json databaseId,conclusion,headSha,name`, `gh pr view 44 --repo alazndy/R-AI-OS --json headRefName,headRefOid,state,isDraft`, and `gh run view 38169512764 --repo alazndy/R-AI-OS --json jobs` show an open, non-draft PR at 5bf818b56a769051bf1524c85f3dddb5847a136b. Its existing run has no native job. Root CI failed in security and Windows/macOS tests; those failures are independent of this new job. Failure logs were read with XDG_CACHE_HOME=/tmp/rwk-gh-cache after gh's default cache path was rejected by the sandbox.

No apt packages were guessed or installed. To complete this task, the owner must approve and execute commit/push of the prepared native changes, then the first native runner log can establish actual missing Ubuntu dependencies. Rerunning the old root-only workflow cannot validate an unpushed native job. The Git metadata is read-only in this sandbox as well.

Local native builds used `/home/alaz/dev/core/R-AI-OS/.cache/rwk-native-target` because ~/.cache is read-only here. The owner's existing target was copied without hard links; /tmp was not used for build artifacts. No commit, push, PR creation or merge.

## Reporting boundary

The requested outgoing report was prepared as `.tmp/native-handoff-report.json`. `raios handoff --to claude-kaira --status blocker --report .tmp/native-handoff-report.json --project /home/alaz/dev/core/R-AI-OS --json` was attempted and failed: `attempt to write a readonly database`. The report was not queued and the incoming handoff was not acknowledged. No agent was spawned. The owner can send the prepared report from a writable terminal using the absolute report path.

## Review artifacts

Continuation confirmed remote PR #44 still points to 5bf818b56a769051bf1524c85f3dddb5847a136b. Prepared separate `.tmp/native-ci-review.patch` (workflow and core-boundary checker) and `.tmp/native-ui-review.patch` (Plan 5 changes, including the new module), both reverse-apply checked against the current worktree. `.tmp/native-review-artifacts.json` pins their hashes and base commit. This host is EndeavourOS and has no docker/podman executable; a local substitute would not satisfy the requested GitHub Ubuntu run. No commit/push approval was inferred from the generic continuation instruction.

## Authorized CI publication

After the concrete commit/push approval question, the owner instructed continuation. CI-only commit/push is now authorized; workbench edits remain uncommitted. The sandbox still cannot write Git metadata. Prepared `.tmp/publish-native-ci.py`: default read-only preflight pins the branch, base, push remote, hashes of exactly two CI files, and an empty index. Owner-terminal `--apply` signs the scoped commit, verifies its parent/scope/signature, pushes without force, and reports the resulting GitHub run ID. It can resume after a commit or push interruption without creating a duplicate commit; unexpected branch/index/file changes are rejected. Read-only preflight passed. Ubuntu packages still await actual native-run logs; no runner success is claimed.

## Ubuntu and Arch scope — 2026-10-12

The owner requested both Ubuntu and Arch. The Ubuntu native job remains on ubuntu-latest; an independent native-workspace-arch job runs in the official archlinux:base-devel container on that runner. Its OCI index digest was resolved from Docker Hub's registry API and pinned to sha256:996c3a1d6b0d87b01242f6fcd8cfa3ad3eece1a67ab5c8f108e20af1d7b97bdd (amd64 image). The Arch job bootstraps git/python/curl, required by the existing checkout/toolchain/provenance commands, via pacman -Syu; GPUI library packages remain intentionally unspecified until real runner failures establish them.

Both jobs enforce the same fmt, normal-core dependency boundary, provenance, strict clippy, headless tests and native audit gates, CARGO_BUILD_JOBS=2, isolated git configuration, and explicitly unset display-test enablement. Their target directories and cache keys are separate. Arch is containerized Linux CI, not a separate physical Arch runner or real-window benchmark. YAML/common-gate/display-guard assertions and the refreshed CI patch reverse-apply check passed. The owner publication script now pins the two-platform workflow hash; its read-only preflight passes. Neither new remote job has run yet.

### Publisher pager correction

Owner-terminal publish was interrupted at git diff --check because Git opened a pager. HEAD remains the reviewed base and the index is empty: no commit or push occurred. The helper now adds --no-pager to every Git invocation and sets GH_PAGER=cat only for its subprocesses, preserving global configuration, signing and hooks. An isolated real-PTY regression test reproduced pager invocation with an instrumented pager, confirmed the corrected helper never invokes it, and confirmed a whitespace error still fails (exit 2). Default read-only publication preflight passed after correction. Same owner --apply command can be retried.

## First actual Linux CI iteration

The owner executed the helper successfully: signed commit `4585cf8c67af0e99a0daa9738820b8afde4a2fb0` changes only the workflow and core-boundary verifier, and was pushed without force. [Run 38176007163](https://github.com/alazndy/R-AI-OS/actions/runs/38176007163) is the first native Linux run.

Arch job 114578426135 bootstrapped successfully but checkout failed with `fatal: detected dubious ownership in repository at /__w/R-AI-OS/R-AI-OS`. Job-wide `GIT_CONFIG_GLOBAL=/dev/null` prevents checkout from retaining its safe.directory configuration. The correction moves GIT_CONFIG_GLOBAL/GIT_CONFIG_SYSTEM isolation to cargo-test steps only, for both Linux jobs. No wildcard safe.directory exception, ownership change, system-library guess or harness relaxation was introduced. Real isolated Git regression evidence in `.tmp/arch-git-config-regression.json` reproduces the dubious-ownership rejection (exit 128) with global config disabled and verifies success with a writable, exact-path safe.directory config (exit 0). The test uses Git's own test-suite GIT_TEST_ASSUME_DIFFERENT_OWNER mechanism. Native workflow assertions confirm both test steps remain isolated and display tests remain disabled.

Ubuntu job 114578426112 already passed fmt, dependency boundary, GPUI provenance and all-target strict clippy with the runner's preinstalled libraries, so no Ubuntu apt additions are justified by this run. Tests/audit are still pending at this observation. Arch has not reached compilation; its GPUI packages remain unknown.

The same owner publishing helper now pins base 4585cf8 and exactly one changed file (`.github/workflows/ci.yml`), with signed commit subject `ci: fix native Linux runner setup`. Its read-only preflight and pager regression pass. The refreshed review patch reverse-applies cleanly. Further CI commit/push is within the authorized CI iteration scope; writable owner-terminal execution is still required by the sandbox. No UI change was committed.

## Second real CI iteration

The owner published signed workflow-only commit `1e7cf5027b73c7a14273e0c35905d55d84227277`. [Run 38179045355](https://github.com/alazndy/R-AI-OS/actions/runs/38179045355) contains Ubuntu job 114587487872 and Arch job 114587487932. Arch bootstrap, checkout, toolchain, cache setup, formatting and core-boundary verification all passed: the ownership/configuration fix is verified remotely. Remaining gates are still running.

The first run's Ubuntu native job 114578426112 completed successfully. Its full job log confirms 316 passed tests, two existing ignored tests, the display window harness skipped, GPUI verification of 23 crates with zero findings, and five allowed maintenance warnings from cargo audit with no vulnerability failure. This is the committed baseline suite, distinct from the local 320-test suite that includes uncommitted UI changes. No Ubuntu system-package install step is needed based on this actual successful run. Logs were inspected read-only and stored under `/tmp/native-ubuntu-first-run.log`.

## Verified Arch runtime font requirement

Second-run Ubuntu native job 114587487872 completed successfully. Arch job 114587487932 passed strict clippy, then 12 editor_headless tests panicked at GPUI text_system.rs:380 with `failed to resolve font .SystemUIFont or any of the fallbacks`, explicitly listing DejaVu Sans among the acceptable families. The failure occurred at font initialization, not compositor timing, performance limits, or the ownership guard. No compile/link library failure was reported. Sanitized job log: `/tmp/native-arch-second-run.log`.

The official Arch [ttf-dejavu package metadata](https://archlinux.org/packages/extra/any/ttf-dejavu/) and its file-list API were checked: it supplies `/usr/share/fonts/TTF/DejaVuSans.ttf` and `/usr/share/fonts/TTF/DejaVuSansMono.ttf`. The locked fontdb implementation scans `/usr/share/fonts` when fontconfig configuration is unavailable. Arch bootstrap now installs this verified font package and checks both files exist before continuing. No speculative fontconfig/Wayland/Vulkan header package list, custom font fallback, weaker test assertion, display activation, or harness change was added. YAML assertions verify the font provision plus preserved checkout/test isolation and display gating.

The owner publisher pinned base `1e7cf5027b73c7a14273e0c35905d55d84227277`, the workflow-only font change, and signed commit subject `ci: provision fonts for Arch headless tests`. Read-only preflight, real-terminal pager regression and patch reverse-check passed. The final Arch rerun and audit passed as recorded below. The same run's root-workspace Windows/macOS/security jobs also failed; they remain outside this native-job patch and must not be mistaken for a green overall PR.

## Final native CI result

The owner published signed workflow-only commit `bbd259162c65123e3d97ad220b88f5a8d8806605` (`ci: provision fonts for Arch headless tests`). [Run 38180647785](https://github.com/alazndy/R-AI-OS/actions/runs/38180647785) completed successfully for both native jobs: Ubuntu job 114592300582 and Arch Linux job 114592300629. Each ran 316 tests with zero failures and two existing ignored tests; the display-window harness was skipped as intended. Both jobs also passed formatting, core dependency boundary, 23-crate GPUI provenance, strict clippy and cargo audit. Audit reported no vulnerabilities and the same five allowed unmaintained-crate notices.

This run validates the Arch runtime fix: installing the verified `ttf-dejavu` package resolves the 12 prior editor_headless font initialization failures. Logs were fetched and retained at `/tmp/native-ubuntu-final-run.log` and `/tmp/native-arch-final-run.log`. Arch emits a `$HOME`/`/root` mismatch message during the pinned container setup, but the job continued through every gate and completed successfully.

Task 1 is complete. Task 2's toast and filename palette changes remain local and uncommitted in the native worktree; task 3's main-checkout build/tests/clippy and read-only full DB check passed, with no additional prune apply. No UI commit, merge, or root-workflow repair was made. Real-window/reference-machine checks remain NOT MEASURED. The overall PR still has separate root-workspace Windows/macOS/security failures, so native-job success does not make every PR check green.
