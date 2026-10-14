# Task 3 — Main checkout and shared database validation

Status: requested build/test/clippy and read-only database checks passed. No new destructive DB action.

Historical applied-prune evidence `.tmp/codex-cache-maintenance-2026-10-10/live-prune.json` (2026-10-10 09:12:57 UTC) records:

| Cache | Removed files | Removed postings | File reasons |
|---|---:|---:|---|
| BM25 | 579 | 1,680,319 | 373 excluded directories, 206 missing |
| Trigram | 803 | 2,333,211 | 428 excluded directories, 375 missing |

These are the already-applied prune totals, not fresh deletions in this task. Subsequent BM25 normalization has its separate backup/verification report.

Commands in the main checkout:

```sh
raios db check --full --json
raios db prune-search --json
CARGO_BUILD_JOBS=2 cargo build --workspace --locked
CARGO_BUILD_JOBS=2 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null cargo test --workspace --locked
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Full DB check returned healthy=true, integrity_messages=[ok], no foreign-key violations, five managed snapshots. At the check's snapshot, main DB bytes=1,686,069,248 and WAL bytes=192,103,272. These are observations from a live database and can change with indexing; they are not the earlier normalization sizes.

Current prune preview (2026-10-11 20:26:21 UTC) has zero BM25 candidates and one missing trigram file with 2,481 postings. No --apply was run, so this preview removed nothing. No new backup was needed for these read-only checks; the verified pre-normalization backup remains preserved.

Workspace build passed. Tests: 1,269 passed, zero failures, two existing ignored doctests. Strict all-target/all-feature clippy passed. No further main-checkout source repair was required. Evidence: `.tmp/handoff-db-full-check.json`, `.tmp/handoff-prune-preview.json`, `.tmp/handoff-main-build.log`, `.tmp/handoff-main-tests.log`, `.tmp/handoff-main-clippy.log`.

Existing uncommitted crates changes were preserved and not committed. README/SIGMAP remain report-only; the new BM25 storage/maintenance commands and session/handoff behavior still need their documentation pass. No live database SQL, delete, checkpoint, normalization retry, or binary replacement was performed in this task.
