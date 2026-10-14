# Search cache maintenance — 2026-10-10

Owner approved the previously proposed repair with “çözüme başla”. Maintenance is
complete. Final production search checks pass; the database is below its unchanged
size cap and the updated daemon is running under the user process manager.

## Live result

- Before maintenance: **2,733,477,888 bytes (2.546 GiB)**.
- Immediately after VACUUM: **2,240,159,744 bytes (2.086 GiB)**.
- After exercising real `search`/`locate` and their normal cache refresh:
  **2,413,404,160 bytes (2.248 GiB)**; `db_size_over_budget=false`.
- Cap: **2,621,440,000 bytes (2.441 GiB)**, unchanged.
- Final net reduction: **320,073,728 bytes (305.25 MiB)**. Reported final savings
  include the cache population caused by functional verification, rather than
  only the smaller intermediate compacted state.
- Removed 579 BM25 file rows / 1,680,319 postings and 803 trigram file rows /
  2,333,211 postings: **4,013,530 postings total**.
- Exhaustive SQLite integrity check after live compaction: `ok`.
- Existing memory content, IDs, revisions, lineage, Cortex data, control-plane
  records and audit history were preserved. Schema was unchanged by pruning.
- Production hybrid search returned eight results, including the new function's
  source file; exact locate found its declaration at line 46.
- Final global preview: zero ineligible cached files/postings. Final WAL checkpoint
  succeeded (`busy=0`); WAL size became zero. Freelist count was zero after search.

The final pre-apply snapshot is:
`/home/alaz/.config/raios/backups/workspace-20261010T090852434844584Z-a3f3b2b97ad5.db`,
SHA-256 `3c3c89433d8ff432e22d80996b64ae0de0fe245e22a3da34ee0c9f956e8e7b72`.
It was quick-integrity checked and independently hashed before any deletion.
Three managed snapshots remain; none were pruned during this work.

## Concurrent metadata and daemon recovery

Immediately after pruning, protected-table fingerprints matched exactly. During
the lengthy full integrity check, other access updated `last_used_at`/`updated_at`
on 28 `mem_items` rows. The original strict whole-row fingerprint guard stopped
the procedure, restored the previous binaries and attempted to resume the daemon.
Its startup discovery updated only `projects.updated_at` on 88 rows. A field-level
comparison against the final pre-apply backup confirmed no memory content/ID
changes, no removed protected rows, no revision/Cortex/control-plane changes, and
an unchanged existing audit prefix. No old database snapshot was restored.

The short-lived tool process's detached daemon did not survive tool completion.
The validated binaries were re-deployed atomically and their SHA-256 values match
`target/release`. The daemon now runs as a **transient** `aiosd.service` under the
systemd user manager (`ActiveState=active`, PID 104264 at verification); IPC, MCP
and HTTP ports 42069/42070/42071 are reachable. This runtime unit survives the tool
process and has restart-on-failure; it does not install or enable a permanent boot
unit. Existing login/autostart configuration was not changed. Logs for this
instance are in the user journal (`journalctl --user -u aiosd.service`), not the old
file-backed hub log. The normal PID-file diagnostic may call it orphaned from
inside the tool namespace; the systemd state and successful protocol calls are
the authoritative liveness evidence.

## Persistent repair

- Shared BM25/trigram/Cortex scan policy excludes `.firebase`, `storybook-static`
  and `graphify-out`. Similar directory names remain eligible.
- `raios db prune-search` globally previews missing or ineligible cached file
  rows without migrations or writes. It includes deleted temporary roots outside
  the main workspace. `--apply` is required to delete rows.
- Apply scans under an immediate SQLite write transaction, rechecks each path
  before deletion, removes only BM25/trigram files and postings, and commits a
  counts-only `search_cache_pruned` audit event in the same transaction.
- Metadata/permission errors, relative cached paths, missing databases and audit
  failures fail closed. A recovered file is retained on recheck. Repeating apply
  after successful maintenance is a no-op when no new stale files exist.
- Explicit child deletion also handles legacy caches without cascading foreign
  keys. No new schema or dependency was introduced.

Source: `crates/raios-runtime/src/search/cache_maintenance.rs`, shared policy in
`search/indexer.rs`, CLI routing in `crates/raios-surface-cli/src/cli/args.rs` and
`cli/db.rs`. The canonical `raios_core::db::db_path` is now public so this CLI
operation respects the same `RAIOS_DB_PATH` override as backup/check operations.

## Verification before live apply

- Workspace tests: **1,123 passed**, zero failed, two pre-existing ignored doc examples.
- Strict all-target/all-feature Clippy: passed.
- Release workspace build, formatting and crate diff checks: passed.
- New regression evidence covers preview preservation, global deletion of lost
  roots/generated content, unchanged memory/search behavior, audit atomicity,
  metadata error rollback, recovered-file rechecks, absent database refusal,
  oversized files and exact directory-component exclusion.
- Dependency check reported no known Rust CVEs. Scans of the changed runtime/CLI
  crates found no HIGH/CRITICAL issues (each A/95 with an existing LOW fixture/auth
  finding). The full repository scanner's existing HIGH pattern matches are in
  the protected native workspace files, which were not changed by this work.
- No commits, changes to `tools/raios-workspace*` or `docs/superpowers/plans`, or
  edits to README/SIGMAP were performed.

## Offline rehearsal

The verified 2026-10-09 backup was copied into an owner-private disposable database.
The repair removed:

| Cache | File rows | Posting rows |
| --- | ---: | ---: |
| BM25 | 579 | 1,680,319 |
| Trigram | 803 | 2,333,211 |

Content hashes matched before/after pruning and again after VACUUM for `mem_items`,
`mem_nodes`, `mem_lineage`, `cortex_chunks`, `projects`, `cp_tasks`, `cp_agent_runs`,
`cp_wrapper_events`, `cp_artifacts` and `cp_approvals`. Schema and the existing audit
history also matched; only the intended maintenance audit event was appended.

Rehearsal database size: **2,733,383,680 → 2,240,073,728 bytes**
(2.546 → 2.086 GiB). SQLite's exhaustive `integrity_check` returned `ok`.
The configured cap is 2,621,440,000 bytes (2,500 MiB), so no cap increase,
snippet-schema migration or redundant-index removal was necessary.

## Recovery and logs

The native snapshot created before rehearsal is:
`/home/alaz/.config/raios/backups/workspace-20261010T085511139835876Z-dcc752b780d8.db`,
SHA-256 `3895dcf646c816b45daf5328b54db2775f124a625333852ab5e3b26471d5eec7`.
It passed quick integrity and an independent checksum check. The live procedure
took the final verified snapshot listed above after stopping the daemon, before pruning.

Evidence and preservation manifests:
`.tmp/codex-cache-maintenance-2026-10-10/`. Previous installed binaries are retained
there for recovery. Binary replacement is atomic and the procedure resumes the
daemon in its finalizer. It never overwrites workspace.db with an older snapshot.

Full Linux validation only; no remote CI or cross-platform run was performed.
The BM25 snippet normalization proposal and redundant trigram index optimization
remain separate follow-up work; neither was needed for this repair.

Completion was handed back to `claude_kaira` through the native control plane:
task `c9fc6a52-5b0a-4e9c-b685-da0260e1beeb`, approval
`f91724b2-a242-4c51-90ed-30f0324c1541`. The report is pending delivery on the
receiver's next `raios run`/`raios task`; no receiving agent was launched.
