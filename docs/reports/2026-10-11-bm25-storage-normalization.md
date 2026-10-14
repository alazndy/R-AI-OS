# Permanent BM25 storage repair — 2026-10-11

## Implementation

BM25 now stores snippets once per `(file_id,line_no)` in `bm25_lines`, a compact WITHOUT ROWID table with cascading file ownership. Token occurrence records remain in `bm25_postings`; normalized rows retain an empty legacy snippet column. Search reconstructs the same snippet by joining the line table, and continues reading legacy inline snippets. Token multiplicity, file IDs, line numbers and scoring formulas are unchanged. Scoped warm loads read only their file IDs and propagate storage errors rather than silently dropping postings.

Existing populated caches keep inline writes until the owner explicitly enables the `lines_v1` format. Only completely empty caches opt in automatically. `bm25_storage_meta` records this transition. This prevents an ordinary partial reader upgrade from silently changing existing storage while an old daemon remains active. Once enabled, all readers must be updated; old binaries cannot read normalized snippets.

`raios db normalize-bm25` is a read-only preview. `--apply` verifies snippet consistency and file ownership, writes line rows, clears repeated snippet payload, enables the format and appends its audit event in one transaction. Conflicting/missing snippets, foreign-key errors and audit failures abort the operation. Repeating successful conversion is a no-op. Compaction is deliberately separate.

Nested `.worktrees` and `worktrees` are excluded from parent traversal/results across BM25, trigram and scoped Cortex retrieval. Explicit branch-root searches still work. Parent refreshes retain independent worktree cache rows; global pruning also preserves eligible worktree files. Existing worktree rows are not globally erased. Broad asset/vendor exclusions were not introduced: their legitimate searchable content now benefits from normalized storage.

## Full-size rehearsal

The current live DB was captured through SQLite's online backup API into a private workspace snapshot, quick-checked and independently SHA-256 hashed:

- Snapshot: `.tmp/bm25-normalization/before.db`
- SHA-256: `fbab3d1eee156f0b4bfebf5d7ecc284e213a940ec684a5834b30d95bbf01f076`
- Snapshot bytes: **2,414,694,400** (2.249 GiB).
- Compacted rehearsal bytes: **1,668,599,808** (1.554 GiB), including the final format marker checkpoint.
- Observed reduction: **746,094,592 bytes (711.53 MiB)**.
- **10,476,353** token occurrences preserved; **1,725,942** distinct legacy snippet lines.
- Repeated snippet bytes: 744,240,227 → 70,119,205 (643 MiB payload savings; physical compaction also removes table slack).
- Conflicting snippets: zero. Logical token/file/line/snippet comparison differences: zero.
- **49** protected memory, control-plane, Cortex, project and file-metadata tables matched before/after conversion and compaction.
- Existing schema objects and audit prefix were preserved. Foreign-key violation results were unchanged.
- Exhaustive SQLite `integrity_check`: `ok`; idempotence passed.

The full-size legacy payload conversion was rehearsed before adding the explicit opt-in marker. The final release separately activated `lines_v1` on the already-converted copy, with its own atomic audit event. Final-release activation evidence is `final-format-activation.json`. Small-data regression tests exercise the complete legacy conversion plus format transition in one call, including positive search scores and mixed legacy/normalized rows. The final normalized-format copy also passed exhaustive integrity checking, and its audit chain verified 4,196 entries. The rehearsal did not mutate the live DB. See the owner-run live rollout observations below for subsequent deployment status.

## Validation

- Full workspace suite: **1,269 passed**, zero failures, two existing ignored doctests.
- Expanded focused storage tests: four passed (including nonzero ranking, mixed reads, conversion gate, conflict rejection, audit rollback, missing-line refusal and idempotence).
- Parent/worktree regression verifies BM25 and trigram filtering, direct branch search, forced parent refresh preserving branch caches, and read-only maintenance eligibility.
- Strict all-target/all-feature workspace clippy, formatting, whitespace checks and release CLI/daemon build passed.
- Runtime pre-change security scan: A/95, zero high/critical findings. Dependency audit evidence is in `.tmp/bm25-normalization-deps.json`.

Evidence is under `.tmp/bm25-normalization/` and `.tmp/bm25-normalization-*.log`.

## Owner-reviewed rollout

`.tmp/deploy-bm25-normalization.py` defaults to a read-only plan. Its `--apply` option authorizes the actual rollout. It verifies pinned release and installed binary hashes, refuses active old MCP/search readers, verifies every unmanaged hub listener belongs to one owner-controlled process with the pinned installed daemon hash, stages both executables, stops the managed service or sends graceful SIGTERM through a pinned Linux pidfd, takes a fresh verified backup, replaces CLI/daemon together, applies normalization, compacts and checks integrity/audit, then restores the daemon's prior running state. An unmanaged daemon is restarted directly with the updated executable and its original arguments, environment and working directory; no service is installed. A 30-second graceful-stop timeout never escalates to SIGKILL. Unknown listeners or a different executable are rejected before database mutation. It never restores an old DB over current user data. After conversion begins, updated readers and backups are retained on error to avoid rolling back to incompatible binaries.

```sh
python3 /home/alaz/dev/core/R-AI-OS/.tmp/deploy-bm25-normalization.py
# After reviewing/approving the live mutation:
python3 /home/alaz/dev/core/R-AI-OS/.tmp/deploy-bm25-normalization.py --apply
```

The workspace sandbox cannot write installed binaries or the live database. The owner must run the approved rollout from their own terminal. The original handoff explicitly required owner approval before destructive operations; no live mutation was attempted here.

No commits or edits to protected workspace tools/plans, README or SIGMAP. README/SIGMAP should document the new maintenance command, normalized line storage and explicit format gate in the owner's documentation pass.

## Owner-run live rollout observations

The owner ran `--apply`. Deployment evidence is in `.tmp/bm25-normalization/deployment-20261011T185621Z/`. The fresh pre-conversion backup passed quick_check and has SHA-256 `0d1ddbeca6fe365b22638709d61f624b4ff758586087255ca2c0c88021b749b7` (2,414,956,544 bytes). Both installed binaries match the pinned release hashes. Conversion committed `lines_v1`, converted 10,476,353 postings, and reported zero conflicts. The live main DB measures 1,668,923,392 bytes, a reduction of 746,033,152 bytes (~711.47 MiB).

Independent read-only verification: quick_check returned `ok`, zero inline snippets remain, normalized snippet joins return expected source text, and the audit chain passed with 4,195 entries. Hub IPC/MCP/HTTP ports respond. Independent exhaustive `integrity_check` also returned `ok` (155.6 seconds), confirming live database integrity. Evidence is saved separately as `independent-verification.json`; it does not substitute a fabricated deployment result. The deployment's final `result.json` remains missing, so the script's own final completion is unconfirmed and the terminal's final output was requested. CLI search attempted a local index refresh that the workspace sandbox rejected as a read-only database write; this does not verify owner-terminal keyword search behavior.

### Confirmed checkpoint-only failure

The owner supplied the traceback: the original script asserted zero busy status on the **post-VACUUM** `wal_checkpoint(TRUNCATE)` (line 171). Normalization and VACUUM had returned successfully, and the finally handler restarted the unmanaged hub (PID 547604). SQLite could not obtain the access needed to truncate WAL at that moment; an active reader or concurrent checkpointer can cause this, but the exact blocking process was not identified. Independent exhaustive integrity and audit checks subsequently passed. The script now retains the strict pre-compaction checkpoint requirement, but records a busy post-compaction checkpoint as deferred WAL truncation and proceeds to integrity/audit verification. It reports WAL bytes and logical database bytes separately. A real temporary SQLite WAL test reproduced a reader surviving VACUUM, a busy TRUNCATE, preserved data/integrity, and successful truncation after the reader released its snapshot. All six deployment-helper tests passed. No live retry or checkpoint was performed by the agent; rerunning normalization is unnecessary.

### Owner-terminal search verification

The owner ran `raios search "handoff" --top-k 5` from the project root. The CLI reported a resident-daemon response and returned five keyword results with file paths, line numbers and reconstructed snippets, including `dto.rs`, `policy.rs`, a design specification and `secret_scan.rs`. This completes the live keyword-search smoke test previously blocked by the agent sandbox. The first result was a `.tmp/.../handoff.json` artifact; temporary-artifact search pollution is a separate scope-policy improvement candidate, not part of the snippet-normalization change. No exclusion was added during this verification.
