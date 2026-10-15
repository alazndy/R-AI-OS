# General R-AI-OS recovery — 2026-10-10

Author: Codex Kaira. Follow-up to `2026-10-10-general-raios-audit.md`. Source fixes are implemented, validated, and installed. Historical session reconciliation and the orphan contract repair were approved and completed on 2026-10-11 (Europe/Istanbul); see the completion evidence below.

## Installed changes

### ANKA v2 compatibility restored

Integrated the committed ANKA implementation from `anka-reliability-privacy` (HEAD `1c41e3f`) into the current working source through a crate-scoped patch. This restores the v2 envelope, coverage, tombstone migration, privacy consent, bounded importers, provenance checks, publication locks, and native policy/timer commands. Existing handoff and search-cache maintenance changes remain present. Uncommitted changes in the other worktree were not copied or modified. The only lockfile dependency change needed for this integration is adding the already resolved `libc` dependency to `raios-runtime`.

The installed CLI reads all 2,394 existing records and 73 sources as `ready`. The live cache was not rebuilt or modified. Its SHA-256 remains `69f8ce890bcaaadfc6afa8bf962a95d2502f9d056c3c6edcaea2682e8037a7d9`, identical to the recovery copy at `.tmp/codex-general-recovery-2026-10-10/before/anka-index-v2.json` (mode 0600).

Debug, release, and installed-binary smoke tests read a copy of the actual cache, perform a scoped refresh in an isolated HOME/XDG environment, retain the other harnesses' coverage and record counts, and confirm v2 publication. They also confirm that an unknown schema version is rejected without overwriting it. The existing timer service points to `/home/alaz/.local/bin/raios anka index`, which now resolves to the compatible installed binary. Its live refresh was not forced; the existing schedule and settings remain intact.

### Wrapper sessions gain process identity and atomic transitions

`cp_session_start_in_workspace` creates task, contract, run, and optional process witness in one transaction. The additive `cp_wrapper_processes` table stores wrapper and child witnesses containing host, boot, PID namespace, PID, and process-start ticks. PID reuse cannot impersonate the original process. Runtime launch records both processes and uses the effective canonical project directory for the contract, fixing the `--project` scope mismatch.

Completion validates task/run ownership, closes the run and any still-in-progress task transactionally, and includes an audit event. Explicit terminal task decisions are preserved. Repeated completion cannot revive a terminal run. Timeout handling also reaps the killed child.

The daemon scheduler reconciles only sessions carrying process witnesses. A dead wrapper with a live child is retained. Permission failures, another process namespace/host, an uncertain spawn-to-persistence gap, and missing child identity remain unverified rather than being cancelled by elapsed time. Process inspection currently supports Linux; other platforms retain unverified sessions. This is conservative recovery, not a promise to classify every interrupted session automatically.

Native maintenance: `raios db reconcile-sessions` is a read-only preview; `--apply` closes selected records with the same transactional audit boundary. `--include-legacy` adds an explicitly reviewed historical set based on strictly parsed local timestamps at least 24 hours before this boot. That margin excludes timestamps near the boot/clock boundary. Legacy rows are never reconciled automatically by the daemon.

Actual subprocess tests verify normal launch scope, kill the wrapper while preserving its running fake child, then kill the child and exercise preview, apply, and idempotent repeat against an isolated database. No real model was launched.

### Explicit health queries include held projects

Explicit queries use registered entities across statuses and retain nested registered projects. Canonical paths and exact names take priority; partial names remain supported. Unknown or empty selections fail meaningfully rather than succeeding with an empty project list. Default workspace scan filtering is unchanged.

The real absolute-path query now returns R-AI-OS with status `beklemede`. Its native health result also reports the repository's latest CI badge as `failure`; this badge was not attributed to the current working source, and remote CI was not rerun or repaired. Local validation below is separate from that remote badge.

### Doctor reports evidence without claiming backend authentication

Credential discovery no longer treats file modification age as token expiration. OpenCode and Antigravity authentication are unverified when no reliable local probe exists; the retired Gemini token directory is no longer required for the installed Antigravity CLI.

JSON exposes `authentication_verified: false`; local checks leave `tier_reached` at `offline`. Full requests run a bounded, output-suppressed local execution probe (`--help` for `agy`, `--version` for the others), with a five-second timeout and child cleanup. A successful local execution is explicitly distinguished from authenticated backend reachability. Stored legacy tiers are likewise not promoted into authentication evidence. This corrects the diagnostics; it does not validate or change anyone's provider credentials, and no paid model request was made.

### Database health and contract repair expose referential integrity

`raios db check` includes `foreign_key_check`. A physical `quick_check` result of `ok` no longer hides dangling references. JSON returns one structured report and a nonzero exit when unhealthy. Recovery snapshots can still be taken from a physically readable database with foreign-key violations; they preserve and report those violations rather than silently repairing them.

Native `raios db repair-orphan-contracts` previews missing task links. Explicit `--apply` clears only dangling links on contracts referenced by no agent run, implementing the schema's `ON DELETE SET NULL` intent without deleting the contract or its policy/budget fields. The repair and audit commit together; audit failure rolls the repair back, and referenced contracts block the operation. The live repair was applied after owner approval on 2026-10-11.

## Validation and deployment

- `cargo test --workspace --offline -j 2`: 1,247 passed, zero failures, two existing ignored documentation examples.
- Strict all-target/all-feature Clippy, format check, and release workspace build passed; `git diff --check` passed.
- Debug, release, and installed smoke tests passed for existing ANKA cache compatibility, isolated safe publication, unknown-schema refusal, correct project contract scope, live-child preservation, dead-pair recovery, and repeated apply.
- Source-level regression tests cover transactional session start/end failures, ownership-bound child attachment, terminal task decisions, held/nested health selection, honest doctor notes, foreign-key reporting and backup, and audited contract repair/rollback.
- Atomically replaced both installed binaries while retaining their previous versions. Deployment refuses an unexpectedly changed installed binary and checks existing-cache compatibility before replacement.
- Installed SHA-256: `raios` = `bfc9a998ba0ed00d4d071f6c4fc51cf10861f3eb389fdb480beb23c9a180329b`; `aiosd` = `5b1b5961fc4b58c49c98394705949e4c261b6a6ee4a06801057d07bf1e1cdb07`.
- Existing `aiosd.service` restarted active, PID 498441, zero restarts, exit status zero, all three ports ready. Existing persistent boot service remains in place. Logs show normal worker startup and subsequent health/Git scans.
- Audit chain verifies 3,575 entries at the post-install check. A strict handoff label check finds one historical 2026-06-25 record resolved by `test_cleanup`: rejected approval, superseded artifact, cancelled run and task. Every linked execution is terminal; this is not an active/reopened handoff. No historical relabeling was performed. Two genuinely pending delivery records were present at the check.

Evidence: `.tmp/codex-general-recovery-2026-10-10/`, including logs, the integration patch, source recovery copies, installed hashes, three smoke-test summaries, health results, database checks, and maintenance previews.

## Historical pre-approval snapshot — 2026-10-10

Fresh online database backup: `/home/alaz/.config/raios/backups/workspace-20261010T201009393280466Z-214c9d07668d.db`, 2,413,907,968 bytes, physical integrity `ok`, SHA-256 `640f75dab412c8ca6622e36a5e2f4b9a7ed655947de3a1c35d796b8a2b41e796`. Four managed snapshots are retained; none were pruned. Previous installed binaries are retained under `.tmp/codex-general-recovery-2026-10-10/previous-installed-binaries/`.

The owner-reviewable live previews identify:

1. **606 historical wrapper runs**, from 625 inspected. Nineteen live or unverified records are preserved. Applying the preview records failed/interrupted completion; only still-in-progress task rows transition. Already-terminal task decisions remain intact. No session was closed on the live database during this work.
2. **One unreferenced orphan contract**, ID `a206df13-ada2-4d87-8299-4e7ac30b3fa9`, linked to missing task `03f26fc8-4b35-445a-8dc2-0af61561f762`. Detaching the missing link preserves the contract. No live link was changed.

The post-install database is 2,413,981,696 bytes, below the unchanged 2,621,440,000-byte cap. Its physical `quick_check` is `ok`, but `healthy` correctly remains false because the orphan reference awaits repair. After owner approval, rescan both previews, apply through the native commands, then verify zero remaining eligible candidates, foreign keys, and the audit chain. Do not close the remaining unverified records merely to make the running count small.

## Scope preserved

No commit, push, branch switch, worktree merge, or outgoing handoff was made. No changes to `tools/raios-workspace*`, `docs/superpowers/plans`, README, or SIGMAP: all 153 protected baseline file hashes match. Existing user changes are preserved. Project memory, decision notes, and learning entries record this recovery and the approved maintenance outcome.

## Approved live maintenance completed — 2026-10-11

The owner's “devam” approved the two concrete operations above. A fresh host-process preview returned exactly the same 606 candidate run IDs, with 19 records preserved. Before applying, a new online backup was created and verified: `/home/alaz/.config/raios/backups/workspace-20261010T220507254951609Z-e8aecc040df5.db`, 2,414,051,328 bytes, SHA-256 `012cd1211308e9d243d45a440d60c66286dab143e8bfefeb36f88a1c35b2769e`. It captures the pre-repair orphan reference; physical integrity is `ok`. Five snapshots are retained and none were pruned.

- Native `reconcile-sessions --include-legacy --apply` closed all 606 historical wrapper runs. The 19 live or unverified rows remain running. Comparing all 625 task/run pairs against their pre-operation state found no unexpected transitions. The 115 candidate tasks that were queued stayed queued; only in-progress tasks transitioned to failed.
- Native `repair-orphan-contracts --apply` detached the single missing task link and preserved its contract. No other contract repair candidates remain.
- Repeated read-only session and contract previews return empty candidate lists. Two genuinely pending handoffs remain, and the previously documented closed historical label mismatch is unchanged.
- `raios db check --json` exits successfully: physical integrity `ok`, zero foreign-key violations, `healthy: true`. Database size is 2,414,350,336 bytes, below the unchanged 2,621,440,000-byte cap.
- `raios verify-chain --json` verifies all 4,182 entries: the prior 3,575 plus 606 session closure events and one contract repair event.
- Daemon remains active at PID 498441 with zero restarts. All 153 protected file hashes still match. No source changes or new deployment were needed for this approved maintenance.

Evidence under `.tmp/codex-general-recovery-2026-10-10/`: `backup-approved-2026-10-11.json`, `sessions-approved-preview.json`, `session-state-before-approved.json`, `sessions-applied-2026-10-11.json`, `contracts-applied-2026-10-11.json`, `session-state-verification.json`, and the `*-after-approved.json` checks. No cleanup approval item remains from this recovery. Preserve the remaining unverified sessions until sufficient process evidence exists.
