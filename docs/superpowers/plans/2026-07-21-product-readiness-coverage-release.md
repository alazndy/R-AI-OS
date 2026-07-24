# Product Readiness: Coverage + Release Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the two remaining engineering gaps identified in the 2026-07-21 "ürün olmaktan ne kadar uzağız" audit — zero test coverage on high-value pure logic inside `raios-runtime`'s async daemon workers, and no downloadable release binaries attached to GitHub Releases — without touching anything outside those two subsystems.

## Completion — 2026-07-24

- [x] Added focused compliance-scanner and lifecycle-status regression tests.
- [x] Added pinned CI checks, a measured 40% line-coverage floor, locked installer smoke tests, extension package validation, and a signed-tag release workflow with archives, checksums, and provenance attestations.
- [x] Validated locally: 747 Rust tests, strict Clippy, format, %42.35 line coverage, extension package/audit, and Unix installer staging.
- [ ] Run the protected GitHub CI pipeline and publish the signed `v3.7.1` tag (release-control step; requires a verified GitHub SSH signing key).

**Architecture:** Two independent phases, each shippable on its own. Phase 1 (Coverage) follows the extraction pattern already used twice today in this codebase (the `47b8f52` scheduler backoff fix and the `daac86f` `ENV_LOCK` race fix): pull pure decision/parsing logic out of I/O-heavy or async-loop functions into standalone synchronous functions, then unit-test those directly — never invent a new testing pattern when this repo already has one that works. Phase 2 (Release Packaging) adds one new GitHub Actions workflow, triggered on `v*` tag push, that cross-compiles release binaries on all three CI matrix OSes and attaches them to the matching GitHub Release via `softprops/action-gh-release`.

**Tech Stack:** Rust 2021 (workspace: `raios-core`, `raios-runtime`), Tokio, GitHub Actions, `softprops/action-gh-release@v2`.

## Global Constraints

- Every task ends with `cargo test --workspace` (all crates, not `--lib` only) at 0 failures and `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean — this is the bar every prior commit today (`47b8f52`, `b954c1f`, `3ab477d`, `daac86f`) was held to; don't lower it.
- Test module style: inline `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of the file being tested — this is the established convention in both `raios-core` and `raios-runtime` (see `agent_runner.rs`, `hooks.rs`, `db/scheduler.rs`).
- Extracted pure functions keep the exact existing behavior — these are coverage/refactor tasks, not behavior-change tasks. Any behavior difference from the current inline logic is a bug in the task, not a feature.
- One commit per task, `Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>` trailer, following the message style already used in this session's commits (`git log --oneline -10` on `master` for reference).
- Do not touch `raios-surface-tui`'s `ui/panels/*.rs` or `app/events/*.rs` rendering code in this plan — that 0%-coverage mass needs golden-render snapshot tests, a different technique, scoped explicitly out of this plan (see "Explicitly Out of Scope" at the end).

---

## Phase 1: Coverage — Extract and Test Pure Logic in `raios-runtime`

### Task 1: `compliance.rs` pure pattern-scanner tests

**Files:**
- Modify: `crates/raios-runtime/src/compliance.rs` (add `#[cfg(test)] mod tests` at end of file, after line 246)

**Interfaces:**
- Consumes: existing `pub fn check_file(path: &Path, content: &str) -> ComplianceReport` (line 71), `ComplianceReport::grade()` (line 28), `ComplianceReport::score_color()` (line 38), `ComplianceReport::first_issue()` (line 60). No signatures change in this task — every function under test is already `pub` or reachable via `super::*` from the same file.
- Produces: nothing consumed by later tasks (Task 1 and Task 2 are independent; do in either order).

This file is a 100% pure, zero-I/O, zero-async pattern scanner (secret detection, per-language lint rules, `package.json` package-manager enforcement) — the highest-value, lowest-effort coverage target in the whole 0%-coverage backlog. It currently has no tests at all.

- [ ] **Step 1: Write the failing tests**

Append to the end of `crates/raios-runtime/src/compliance.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_hardcoded_secret_with_key_assign_and_literal() {
        let report = check_file(
            &PathBuf::from("config.rs"),
            "let api_key = \"sk-abc123\";",
        );
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "Possible hardcoded secret/key");
        assert_eq!(report.violations[0].severity, 25);
        assert_eq!(report.score, 75);
    }

    #[test]
    fn does_not_flag_secret_keyword_without_assignment_or_literal() {
        // has the keyword and a literal, but no '=' or ':' — not a plausible
        // assignment, must not false-positive
        let report = check_file(&PathBuf::from("notes.md"), "discuss api_key rotation \"policy\"");
        assert!(report.violations.is_empty());
        assert_eq!(report.score, 100);
    }

    #[test]
    fn ignores_secret_pattern_inside_a_comment_line() {
        let report = check_file(
            &PathBuf::from("config.rs"),
            "// api_key = \"sk-abc123\" (example, not real)",
        );
        assert!(
            report.violations.is_empty(),
            "commented-out lines starting with // must be skipped by check_secrets"
        );
    }

    #[test]
    fn rust_unwrap_outside_test_is_flagged_but_inside_test_is_not() {
        let report = check_file(&PathBuf::from("lib.rs"), "let x = foo().unwrap();");
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "Prefer ? over .unwrap()");

        let report_test = check_file(
            &PathBuf::from("lib.rs"),
            "#[test] fn t() { let x = foo().unwrap(); }",
        );
        assert!(
            report_test.violations.is_empty(),
            "a line containing #[test] must be exempt from the unwrap() rule"
        );
    }

    #[test]
    fn rust_panic_and_todo_both_flagged_on_same_line_independently() {
        let report = check_file(&PathBuf::from("lib.rs"), "panic!(\"boom\"); // TODO: fix");
        let rules: Vec<&str> = report.violations.iter().map(|v| v.rule).collect();
        assert!(rules.contains(&"Avoid panic! — return Result instead"));
        assert!(rules.contains(&"Unresolved TODO/FIXME/HACK"));
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn typescript_any_and_console_log_are_flagged() {
        let report = check_file(
            &PathBuf::from("app.ts"),
            "const x: any = console.log(y);",
        );
        let rules: Vec<&str> = report.violations.iter().map(|v| v.rule).collect();
        assert!(rules.contains(&"Avoid `any` — use unknown + type guard"));
        assert!(rules.contains(&"Remove console.log from production"));
    }

    #[test]
    fn python_bare_except_is_flagged_but_typed_except_is_not() {
        let bare = check_file(&PathBuf::from("app.py"), "except:");
        assert_eq!(bare.violations.len(), 1);
        assert_eq!(bare.violations[0].rule, "Bare except: — specify exception type");

        let typed = check_file(&PathBuf::from("app.py"), "except ValueError:");
        assert!(typed.violations.is_empty());
    }

    #[test]
    fn package_json_flags_npm_and_yarn_but_allows_pnpm() {
        let npm = check_file(
            &PathBuf::from("package.json"),
            "{\"scripts\": {\"start\": \"npm install\"}}",
        );
        assert!(npm
            .violations
            .iter()
            .any(|v| v.rule == "Use pnpm not npm (MASTER.md)"));

        let pnpm = check_file(
            &PathBuf::from("package.json"),
            "{\"scripts\": {\"start\": \"pnpm install\"}}",
        );
        assert!(pnpm.violations.is_empty());
    }

    #[test]
    fn score_never_underflows_below_zero_with_many_violations() {
        // 5 unwrap()s at severity 3 each = 15, well under 100, but this
        // guards the saturating_sub/min(100) clamp path explicitly.
        let content = ".unwrap();\n".repeat(40); // 40 * 3 = 120 > 100
        let report = check_file(&PathBuf::from("lib.rs"), &content);
        assert_eq!(report.score, 0);
    }

    #[test]
    fn grade_and_score_color_thresholds() {
        let mut report = check_file(&PathBuf::from("clean.rs"), "fn main() {}");
        assert_eq!(report.score, 100);
        assert_eq!(report.grade(), "A");
        assert_eq!(report.score_color(), 0);

        report.score = 65;
        assert_eq!(report.grade(), "D");
        assert_eq!(report.score_color(), 1);

        report.score = 10;
        assert_eq!(report.grade(), "F");
        assert_eq!(report.score_color(), 2);
    }

    #[test]
    fn first_issue_formats_line_number_or_falls_back_to_rule_only() {
        let report = check_file(&PathBuf::from("lib.rs"), "let x = foo().unwrap();");
        assert_eq!(
            report.first_issue(),
            Some("Ln   1: Prefer ? over .unwrap()".to_string())
        );

        let pkg_report = check_file(
            &PathBuf::from("package.json"),
            "{\"scripts\": {\"start\": \"npm i\"}}",
        );
        // check_package_json pushes violations with line: 0
        assert_eq!(
            pkg_report.first_issue(),
            Some("Use pnpm not npm (MASTER.md)".to_string())
        );
    }

    #[test]
    fn unknown_extension_only_runs_secret_check_not_language_rules() {
        // a .unwrap() in a .xyz file must not be flagged — check_rust only
        // runs for FileType::Rust
        let report = check_file(&PathBuf::from("data.xyz"), "value.unwrap()");
        assert!(report.violations.is_empty());
        assert_eq!(report.file_type, FileType::Other);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/alaz/dev/core/R-AI-OS && ~/.cargo/bin/cargo test -p raios-runtime compliance:: 2>&1 | tail -20`
Expected: compile error — `ComplianceReport` fields/methods used above (`file_type`, `FileType::Other` etc.) already exist in the real file, so this should actually compile and the tests should mostly PASS immediately (this task adds tests for existing, already-correct behavior — it is coverage-backfill, not a bug fix, so there is no red step in the classic TDD sense). If any test fails, that means the plan's expected values are wrong for the *actual* current code — stop and re-read the real `compliance.rs` source at that exact line before changing the test, do not change the implementation to match a guessed expectation.

- [ ] **Step 3: Confirm all pass and check coverage moved**

Run: `~/.cargo/bin/cargo test -p raios-runtime compliance:: 2>&1 | tail -5`
Expected: `test result: ok. 12 passed; 0 failed;`

- [ ] **Step 4: Full workspace verification**

Run:
```bash
cd /home/alaz/dev/core/R-AI-OS
~/.cargo/bin/cargo test --workspace 2>&1 | grep -E "^test result:|FAILED"
~/.cargo/bin/cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1 | tail -5
```
Expected: every `test result:` line says `0 failed`; clippy ends with `Finished` and no `warning:`/`error:` lines.

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS
git add crates/raios-runtime/src/compliance.rs
git commit -m "$(cat <<'EOF'
test: cover compliance.rs pattern-scanner (0% -> tested)

compliance.rs is a pure, zero-I/O, zero-async pattern scanner used for
per-language lint/secret checks. It had no tests despite being
security-adjacent (secret detection). Adds 12 tests covering secret
detection (incl. comment-line and false-positive exemptions), all four
per-language rule sets, package.json pnpm enforcement, the score
clamp, grade/color thresholds, and first_issue() formatting.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Extract and test `lifecycle.rs`'s pure status-transition logic

**Files:**
- Modify: `crates/raios-runtime/src/daemon/lifecycle.rs:79-100` (extract inline logic into a new function), append `#[cfg(test)] mod tests` at end of file.

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: `fn next_lifecycle_status(current: &str, age_secs: u64, standby_secs: u64, archive_secs: u64) -> Option<&'static str>` — a private (no `pub`) function local to this file, not consumed by any other task in this plan. If a later, separate plan wants to reuse it (e.g. from a CLI dry-run command), it can be made `pub` and moved to `raios-core` at that time — out of scope here.

The existing `start_lifecycle_worker` async loop (lines 16-143) inlines its entire status-transition decision (which project auto-archives, which reactivates) directly in the loop body at lines 79-100, mixed with DB I/O and broadcast messaging. This is the exact same shape of bug class fixed today in `47b8f52` (`cp_scheduled_job_revert_firing`) — decision logic buried inside an untestable async loop. Extracting it here is directly following that fix's pattern, applied prophylactically before it causes an incident instead of after.

- [ ] **Step 1: Write the failing test**

In `crates/raios-runtime/src/daemon/lifecycle.rs`, the function does not exist yet, so this test won't compile — that is this task's red step. Append at the end of the file (after `last_commit_timestamp`, which currently ends the file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    #[test]
    fn active_with_recent_commit_stays_active() {
        assert_eq!(
            next_lifecycle_status("active", 1 * DAY, 14 * DAY, 90 * DAY),
            None
        );
    }

    #[test]
    fn beklemede_with_fresh_commit_reactivates() {
        assert_eq!(
            next_lifecycle_status("beklemede", 1 * DAY, 14 * DAY, 90 * DAY),
            Some("active")
        );
    }

    #[test]
    fn archived_with_fresh_commit_reactivates() {
        assert_eq!(
            next_lifecycle_status("archived", 1 * DAY, 14 * DAY, 90 * DAY),
            Some("active")
        );
    }

    #[test]
    fn active_past_standby_but_before_archive_goes_to_beklemede() {
        assert_eq!(
            next_lifecycle_status("active", 20 * DAY, 14 * DAY, 90 * DAY),
            Some("beklemede")
        );
    }

    #[test]
    fn beklemede_still_within_archive_window_has_no_change() {
        assert_eq!(
            next_lifecycle_status("beklemede", 20 * DAY, 14 * DAY, 90 * DAY),
            None
        );
    }

    #[test]
    fn active_past_archive_window_goes_straight_to_archived() {
        assert_eq!(
            next_lifecycle_status("active", 100 * DAY, 14 * DAY, 90 * DAY),
            Some("archived")
        );
    }

    #[test]
    fn already_archived_past_archive_window_has_no_change() {
        assert_eq!(
            next_lifecycle_status("archived", 100 * DAY, 14 * DAY, 90 * DAY),
            None
        );
    }

    #[test]
    fn boundary_age_exactly_equal_to_standby_secs_is_not_recent() {
        // age_secs < standby_secs is false at equality, so this must fall
        // into the beklemede branch, not stay "recent".
        assert_eq!(
            next_lifecycle_status("active", 14 * DAY, 14 * DAY, 90 * DAY),
            Some("beklemede")
        );
    }

    #[test]
    fn boundary_age_exactly_equal_to_archive_secs_is_archived() {
        assert_eq!(
            next_lifecycle_status("active", 90 * DAY, 14 * DAY, 90 * DAY),
            Some("archived")
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cd /home/alaz/dev/core/R-AI-OS && ~/.cargo/bin/cargo test -p raios-runtime daemon::lifecycle:: 2>&1 | tail -15`
Expected: `error[E0425]: cannot find function `next_lifecycle_status` in this scope`

- [ ] **Step 3: Extract the function and wire the loop to call it**

Replace the inline block in `crates/raios-runtime/src/daemon/lifecycle.rs` (the `let new_status = if age_secs < standby_secs { ... } else { ... };` block currently at lines 79-100) with a call to the new extracted function, and add the function itself right after `start_lifecycle_worker`'s closing brace (before `last_commit_timestamp`):

```rust
            let new_status = next_lifecycle_status(current, age_secs, standby_secs, archive_secs);
```

```rust
fn next_lifecycle_status(
    current: &str,
    age_secs: u64,
    standby_secs: u64,
    archive_secs: u64,
) -> Option<&'static str> {
    if age_secs < standby_secs {
        if matches!(current, "beklemede" | "archived") {
            Some("active")
        } else {
            None
        }
    } else if age_secs < archive_secs {
        if current != "beklemede" {
            Some("beklemede")
        } else {
            None
        }
    } else if current != "archived" {
        Some("archived")
    } else {
        None
    }
}
```

This is a pure refactor — the logic is byte-for-byte the same decision tree, just named and callable outside the loop. Do not change the behavior while doing this step.

- [ ] **Step 4: Run tests to verify they pass**

Run: `~/.cargo/bin/cargo test -p raios-runtime daemon::lifecycle:: 2>&1 | tail -15`
Expected: `test result: ok. 9 passed; 0 failed;`

- [ ] **Step 5: Full workspace verification**

Run:
```bash
cd /home/alaz/dev/core/R-AI-OS
~/.cargo/bin/cargo test --workspace 2>&1 | grep -E "^test result:|FAILED"
~/.cargo/bin/cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1 | tail -5
```
Expected: `0 failed` everywhere, clippy clean. Also manually re-read the modified `start_lifecycle_worker` loop body once to confirm the extraction didn't change which branch fires for any case — this file drives real automatic project-status transitions on a live daemon, a silent behavior change here would misfile real projects as archived/active.

- [ ] **Step 6: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS
git add crates/raios-runtime/src/daemon/lifecycle.rs
git commit -m "$(cat <<'EOF'
refactor: extract lifecycle.rs status transition into a tested pure fn

start_lifecycle_worker's entire active/beklemede/archived decision was
inlined in the async loop body (0% coverage, untestable without a real
tokio runtime + DB + populated project state). Extracted the pure
decision tree into next_lifecycle_status(current, age_secs,
standby_secs, archive_secs) -> Option<&'static str>, unchanged
behavior, now covered by 9 tests including both interval boundaries
(age_secs == standby_secs, age_secs == archive_secs).

Same "pull the decision logic out of the untestable async loop"
pattern already used today in the JSON Backup scheduler fix (47b8f52)
— applied here before it causes a real incident, not after.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Phase 2: Release Packaging — Attach Binaries to GitHub Releases

### Task 3: Cross-platform release-asset workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: nothing from Phase 1 (fully independent — can be done first, last, or in parallel by a different worker).
- Produces: a `release` GitHub Actions workflow. No Rust code interfaces.

Right now `v3.7.0`'s GitHub Release (published earlier today via `gh release create`) has no binary assets — anyone who wants to run R-AI-OS must clone and `cargo build --release` themselves. This task adds a workflow that, on every `v*` tag push, builds release binaries on all three OSes (the same matrix already proven green in `ci.yml` as of commit `daac86f`) and uploads them as downloadable Release assets.

- [ ] **Step 1: Write the workflow file**

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'
  workflow_dispatch:
    inputs:
      tag:
        description: 'Existing tag to attach binaries to (e.g. v3.7.0)'
        required: true
        type: string

env:
  CARGO_TERM_COLOR: always

jobs:
  build:
    name: Package (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-latest
            target_name: linux-x86_64
            bin_ext: ''
            archive_cmd: tar
          - os: macos-latest
            target_name: macos-x86_64
            bin_ext: ''
            archive_cmd: tar
          - os: windows-latest
            target_name: windows-x86_64
            bin_ext: '.exe'
            archive_cmd: zip
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Build release binaries
        run: cargo build --release --workspace --locked

      - name: Package archive (tar.gz)
        if: matrix.archive_cmd == 'tar'
        shell: bash
        run: |
          set -euo pipefail
          STAGE="raios-${{ matrix.target_name }}"
          mkdir "$STAGE"
          cp target/release/raios "$STAGE/"
          cp target/release/aiosd "$STAGE/"
          cp README.md LICENSE CHANGELOG.md "$STAGE/"
          tar czf "$STAGE.tar.gz" "$STAGE"

      - name: Package archive (zip)
        if: matrix.archive_cmd == 'zip'
        shell: pwsh
        run: |
          $stage = "raios-${{ matrix.target_name }}"
          New-Item -ItemType Directory -Path $stage
          Copy-Item "target/release/raios.exe" -Destination $stage
          Copy-Item "target/release/aiosd.exe" -Destination $stage
          Copy-Item "README.md","LICENSE","CHANGELOG.md" -Destination $stage
          Compress-Archive -Path $stage -DestinationPath "$stage.zip"

      - name: Upload build artifact
        uses: actions/upload-artifact@v4
        with:
          name: raios-${{ matrix.target_name }}
          path: |
            raios-${{ matrix.target_name }}.tar.gz
            raios-${{ matrix.target_name }}.zip
          if-no-files-found: ignore

  publish:
    name: Attach to Release
    needs: build
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: dist
          merge-multiple: true
      - name: List staged assets
        run: ls -la dist/
      - uses: softprops/action-gh-release@v2
        with:
          tag_name: ${{ inputs.tag || github.ref_name }}
          files: dist/*
          fail_on_unmatched_files: true
```

- [ ] **Step 2: Verify the workflow YAML is well-formed before pushing**

Run: `cd /home/alaz/dev/core/R-AI-OS && python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo "VALID YAML"`
Expected: `VALID YAML`. If this fails, fix the indentation/syntax error it reports before continuing — do not push invalid YAML and debug via failed Actions runs.

- [ ] **Step 3: Commit and push**

```bash
cd /home/alaz/dev/core/R-AI-OS
git add .github/workflows/release.yml
git commit -m "$(cat <<'EOF'
feat: add cross-platform release-asset workflow

v3.7.0's GitHub Release currently has no downloadable binaries —
anyone who wants to run R-AI-OS must clone and build from source.
Adds a release.yml workflow (tag push on v* or manual
workflow_dispatch with an existing tag) that builds raios+aiosd on
the same three-OS matrix already proven green in ci.yml, packages
each as a tar.gz (Linux/macOS) or zip (Windows) bundled with
README/LICENSE/CHANGELOG, and attaches them to the matching GitHub
Release via softprops/action-gh-release.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
git push origin master
```

- [ ] **Step 4: Retroactively attach binaries to the existing v3.7.0 release**

The `v3.7.0` tag already exists and its Release already exists (published earlier today) — this workflow only runs automatically on a *new* tag push, so v3.7.0 needs one manual trigger:

```bash
gh workflow run release.yml --repo alazndy/R-AI-OS -f tag=v3.7.0
```

- [ ] **Step 5: Watch the run and verify assets landed**

```bash
sleep 15
RUN_ID=$(gh run list --repo alazndy/R-AI-OS --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$RUN_ID" --repo alazndy/R-AI-OS --exit-status
gh release view v3.7.0 --repo alazndy/R-AI-OS --json assets --jq '.assets[].name'
```
Expected: the watch command exits 0 (success), and the final command lists 3 files — `raios-linux-x86_64.tar.gz`, `raios-macos-x86_64.tar.gz`, `raios-windows-x86_64.zip`.

- [ ] **Step 6: Update memory.md**

Following this repo's `AGENT_CONSTITUTION.md` rule to log every major change, add one line to `memory.md`'s Active Objectives and one Change Log entry noting the release workflow now exists and v3.7.0 has downloadable binaries, then regenerate `SIGMAP.md` (`sigmap` from repo root) and commit both together.

---

## Explicitly Out of Scope (do not build tasks for these without a fresh investigation pass)

These were identified during the 2026-07-21 audit but are **not** included as tasks above, either because they need real investigation before they can be specced with complete, non-placeholder code (per this plan's own rules), or because they aren't solvable by a code plan at all:

- **Remaining 0%-coverage `raios-runtime` files not covered by this plan**: `daemon/sentinel.rs`, `daemon/git.rs`, `daemon/scheduler.rs`'s own async loop (only the DB-layer `cp_scheduled_job_revert_firing` got tested today, in `47b8f52`), `health.rs`, `kernel.rs`, `workers.rs`, `discovery.rs`, `sync.rs`, `tasks.rs`, `daemon/cmd/*.rs` (6 files), `server/http/*.rs` (3 files), `filebrowser/*.rs`, `cortex/chunker.rs`, `system_scan/tools.rs`. Several of these (`tasks.rs`'s `parse_task_line`/`serialize`, `discovery.rs`'s `scan_dir_for_skills`) likely have the same easily-extractable pure-logic shape as this plan's Task 2 — worth a follow-up plan once Phase 1 here is merged and the pattern is proven twice over.
- **`raios-surface-tui`'s 0%-coverage rendering files** (`ui/panels/*.rs`, `app/events/*.rs`, the bulk of the workspace's coverage gap by line count): needs golden-render snapshot tests (the existing `ui/routes/tests::golden_render_*` pattern), not line-coverage unit tests. Different technique, different plan.
- **`raios-core/src/safe_io.rs`**: rejected today as an easy win (see `memory.md`, 2026-07-21 entry) because it unconditionally dials the live daemon's real TCP port with no dependency-injection seam. Needs a design change (an injectable port/connector) before it's testable at all — a design task, not a coverage task.
- **Non-technical onboarding (landing page, demo video, "what is this" copy for non-developers)**: a content/design task, not an engineering task — better suited to a `frontend-design` or copywriting pass than a TDD implementation plan.
- **External traction (stars, forks, real external users)**: not achievable through any code change. Explicitly not a task.

## Self-Review

- **Spec coverage**: Both concrete gaps from the "ürün olmakta ne eksik" audit that are (a) fully specifiable right now and (b) purely engineering work are covered — Phase 1 raises real coverage on genuinely high-value pure logic, Phase 2 closes the "no downloadable binary" gap. The gaps that aren't covered are listed explicitly above with the reason each was excluded, not silently dropped.
- **Placeholder scan**: No "TBD"/"handle edge cases"/"similar to Task N" — every step has complete, real code (compliance.rs tests are against the actual current file, `lifecycle.rs`'s extraction is behavior-preserving by construction, `release.yml` is a complete workflow file).
- **Type consistency**: `next_lifecycle_status`'s signature (`&str, u64, u64, u64) -> Option<&'static str>`) is used identically in Task 2's Step 1 test-writing and Step 3 implementation — matches the real call-site types in `lifecycle.rs` (`current: &str` from `proj.status.as_str()`, `age_secs`/`standby_secs`/`archive_secs`: all `u64` in the existing code at lines 33-34, 76).
