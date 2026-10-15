# General R-AI-OS audit — 2026-10-10

Author: Codex Kaira. Scope: installed CLI, live daemon, workspace database, wrapper lifecycle, health/auth diagnostics, and ANKA compatibility. This investigation follows the completed search-cache maintenance and handoff lifecycle work. Priorities below are local remediation priorities, not vulnerability severity ratings.

## Findings

### 1. P1: the deployed branch cannot read the existing ANKA v2 cache

The installed `raios anka status --json` fails with a corruption error and recommends rebuilding. However, `/home/alaz/.cache/raios/anka/index.json` is valid JSON, 3,744,679 bytes, mode 0600, with `schema_version`, `records_v2`, and `coverage` fields. The previous installed binary, retained at `.tmp/codex-cache-maintenance-2026-10-10/previous-installed-binaries/raios`, reads that same file successfully: 2,394 records from 73 sources, state `ready`.

The checked-out branch still uses the older `AnkaIndex` layout (`records`, `indexed_sources`, `last_indexed_at`) in `crates/raios-runtime/src/anka.rs`. Its `read_index` treats a serialization layout mismatch as corruption. The installed binary was replaced during my earlier maintenance deployment with this checkout's release build; I missed the deployed feature/schema compatibility check. This is a deployment regression, not evidence of damaged transcript data.

The v2 implementation already exists on `anka-reliability-privacy` in `.worktrees/anka-reliability-privacy` (HEAD `1c41e3f`; cache envelope implementation introduced in `b32fd2d`). It includes migration, coverage, importer, provenance, privacy, locking, and publication changes across multiple modules. It cannot safely be reduced to renaming a JSON field. Restoring the old binary alone would also discard the newly deployed handoff/cache-maintenance behavior.

The enabled `raios-anka-index.timer` invokes the currently installed binary through `raios-anka-index.service`. A future refresh with the old writer risks replacing the v2 envelope with the old format and losing the newer metadata and safeguards. The preceding successful timer execution does not validate the replacement binary.

Recommended repair: preserve and back up the v2 cache; reconcile the v2 source implementation with the current handoff/cache changes, validate existing-cache reads and safe writes, then deploy the combined build. Do not run the current CLI's suggested ANKA rebuild against this cache. No ANKA index write or timer configuration change was performed in this audit.

### 2. P1: wrapper sessions survive process death and reboot as running

There are 621 `cp_agent_runs` rows with provider `wrapper` and status `running`; 620 are older than 24 hours. The oldest dates to 2026-06-28. Of these, 617 predate the current host boot (2026-10-09 20:46:09 +03:00), so they cannot describe surviving wrapper processes. Host process inspection found four live wrapper processes.

`crates/raios-core/src/db/wf_sessions.rs` starts a task, contract, and running agent record without persistent PID/start identity, boot identity, or heartbeat. Start/end writes are separate updates. `crates/raios-runtime/src/agent_runner.rs` closes a session on spawn failure or normal completion, but an interrupted wrapper or reboot can bypass the finalizer. No authoritative orphan reconciliation was found in the inspected path.

Recommended repair: bind sessions to boot/process-start identity, add a liveness mechanism, make session transitions transactional, and reconcile proven dead processes through an audited native workflow. Age alone must not close legitimate long-running sessions. No live session rows were modified.

This is distinct from handoff lifecycle: the repaired linked handoff records have zero terminal-state inconsistencies and one genuinely pending fresh Claude delivery.

### 3. P2: explicit project launch persists the wrong contract workspace

An isolated smoke test launched `raios run claude --project B` from directory A with a fake executable. The child ran in B, but `cp_run_contracts.workspace_root` was A. The test used a temporary HOME/XDG/database and launched no real model.

The runner sets the child command's working directory, while `cp_session_start` obtains `std::env::current_dir()` from the parent process. Consequently the selected project and stored run scope can disagree. This can mislead subsequent workspace-bound checks or attribution.

Recommended repair: pass the resolved project root explicitly into the session/contract creation API, canonicalize it consistently, and validate task/project/contract agreement. Reproduction: `.tmp/codex-general-audit-2026-10-10/project-contract-smoke.json`.

### 4. P2: explicit health queries silently omit held projects

Both `raios health R-AI-OS --json` and the explicit absolute-path query return success with `projects: []`. The project exists in the database (ID 4337), with status `beklemede`.

`crates/raios-core/src/entities.rs` excludes held projects during loading/discovery. `crates/raios-surface-cli/src/cli/health.rs` applies the user's explicit selection after this filtering, so the requested project has already disappeared. There is also no meaningful not-found diagnostic in this case.

Recommended repair: explicit name/path queries should resolve across known statuses, while ordinary default workspace scans may retain their active-project filter. Reflection's score is likewise a filtered workspace assessment rather than a complete inventory: this run assessed 35 projects, compared with 216 registered database projects.

### 5. P2: doctor authentication results exceed the available evidence

`crates/raios-runtime/src/system_scan/doctor.rs` treats credential-file presence as authentication evidence, uses file modification age for the Codex warning, and assumes authentication for an unhandled OpenCode branch. The Antigravity probe checks the legacy `~/.gemini/antigravity-cli/` location. The installed `agy` is a modern native CLI; absence of that old directory does not establish that its current authentication is broken.

The full-tier probe runs a version command rather than an authenticated backend operation. Therefore neither a successful full-tier result nor a stale-file warning proves token validity or expiration.

Recommended repair: distinguish installation, local credential discovery, and authenticated reachability; use provider-specific current probes and report an unverified state when a reliable check is unavailable. No paid model run or remote credential validation was attempted, so actual provider login state remains unverified.

### 6. P2: one run contract references a missing task

Read-only `PRAGMA foreign_key_check` reports `cp_run_contracts|62|cp_tasks|0`; an independent missing-parent join confirms one orphan contract. No orphan agent runs or health-cache entries were found in the corresponding checks.

The database's `quick_check` returns `ok`, which does not establish foreign-key consistency. Recommended repair: trace the orphan's provenance, repair through the native control plane with audit preservation, and include foreign-key checks in database health reporting. No direct database repair was attempted.

## Healthy checks and limits

- Workspace database: 2,413,694,976 bytes (approximately 2.248 GiB), below the unchanged 2,621,440,000-byte cap, with 198.12 MiB remaining. `quick_check` is `ok`; three managed backups remain. Search-cache maintenance preview finds zero eligible stale postings/files.
- Audit chain: all 3,574 entries verify. Handoff consistency checks find zero anomalies and one pending outgoing delivery.
- Daemon: `aiosd.service` is active with PID 170565, zero restarts, and all three ports ready. Its live unit is transient, but the persistent `raios-hub.service` exists and is enabled for boot. Sandbox-hidden PID output was checked against host output and is not a daemon defect.
- Recent daemon logs show no repeated crash/panic. A single rejected authentication attempt is not evidence of an authentication bypass. Four active cron jobs have recent executions or a future scheduled occurrence; no repeating failure loop was established.
- The lightweight whole-repo security scanner's two HIGH matches in the protected workspace tools were reviewed: a traversal string is in a negative test fixture, and the flagged SQL interpolation is a constant join with parameterized user values. These findings do not establish exploitable vulnerabilities. This limited scanner review is not a comprehensive security assessment.
- Latest source validation from the preceding implementation remains 1,129 passing tests, strict all-target/all-feature Clippy, formatting, and release workspace build. No source code changed during this audit, so those checks were not repeated. These checks did not cover deployed ANKA cache compatibility.
- Reflection is 67/100 with six missing SIGMAP files and two missing README files; these documentation gaps are reported only. No remote CI, provider backend authentication, or exhaustive dependency security audit was performed here.

Evidence is under `.tmp/codex-general-audit-2026-10-10/`, including `summary.json`, `session-liveness.json`, `anka-metadata.json`, and the isolated contract-scope smoke test. No transcript bodies were needed for the cache diagnosis.

## Scope preserved

This audit changes only this report and project learning/memory notes. It does not repair the six findings, edit source, rewrite caches, clean control-plane rows, alter services/timers, or create another handoff. No commit was made. `tools/raios-workspace*`, `docs/superpowers/plans`, README, and SIGMAP were left untouched; existing changes remain in place.
