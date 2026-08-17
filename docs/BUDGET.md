# workspace.db — Size & Row Budget

Single source of truth for `raios` is one SQLite file: `~/.config/raios/workspace.db`.
It has no partitioning and no automatic vacuuming, so every new table or
high-frequency insert path is permanent growth unless something prunes it.
This is the ratchet checklist for that growth — read-only today, checked by
`raios health` via `db_budget_check()` in
`crates/raios-runtime/src/system_scan/db_budget.rs`.

## Current soft caps

| Table / metric | Soft cap | On exceed |
|---|---|---|
| `mem_items` (per `project_key`) | 5,000 rows | **warn** — `raios health` prints the offending project(s); distillation/pruning should be catching this before it does |
| `workspace.db` total file size | 2.5 GB | **warn** — `raios health` flags `OVER CAP`; nothing blocks yet |
| `cp_tasks` | — (counted only) | none yet — establishing a baseline |
| `cp_agent_runs` | — (counted only) | none yet — establishing a baseline |
| `cp_wrapper_events` | — (counted only) | explicit wrapper-note evidence; bounded to 500 characters per row and retained for run auditability |
| `cp_artifacts` | — (counted only) | none yet — establishing a baseline |
| `audit_log` | — (counted only) | none yet — has its own hash-chain integrity check (`raios verify-chain`), not a size cap |

All caps here are **warn, not block** — `raios health` reports the numbers, it
does not fail the command or refuse to run. Nothing in raios currently
deletes/prunes rows to enforce a cap automatically (the exception is
`cp_log_append`'s existing ring-buffer prune, unrelated to this check).

These numbers are starting points, chosen before real production data was
available (see "measured on 2026-07-15" note below) — expect them to move as
real usage patterns become clear, not to stay fixed forever.

## How it's measured

- Row counts: `SELECT COUNT(*) FROM <table>` per table above.
- Total size: `PRAGMA page_count * PRAGMA page_size` against the open
  connection — reflects the real on-disk file size, including WAL/free
  pages, not just live row bytes.
- Everything here is read-only. No writes, no `VACUUM`, no deletes.

Run it yourself: `raios health` (prints a "DB Budget" section after the
per-project list) or `raios health --json` (adds a top-level `db_budget` key
alongside `projects`).

## Measured on 2026-07-15 (this machine, at the time this doc was written)

`workspace.db` was already **2.2 GB** — well over the 500 MB soft cap — while
every individual project's `mem_items` count was still small (under 100
rows each). This is exactly the kind of gap this check exists to catch: no
single table looked alarming in isolation, but the file as a whole had
already blown past budget. `audit_log` alone was already in the low
thousands of rows on a single dev machine. Treat the initial caps above as
provisional until a follow-up task investigates what's actually consuming
the 2.2 GB (likely candidates: WAL file not checkpointing, Cortex vector
blobs, or `audit_log`/`tool_traces` growth — out of scope for this
read-only reporting task).

## Measured on 2026-08-17 — the 2026-07-15 follow-up

`workspace.db` had grown to **4.4 GB**. It was not WAL/Cortex-blob/audit_log
growth — the dominant consumers were `bm25_postings` (2.2 GB) and four
trigram tables (~1.5 GB combined), i.e. the BM25/trigram search engines
indexing more than intended:

- `/home/alaz/dev/core/R-AI-OS-audit` — a full second clone of this repo made
  for a one-off audit task over a month earlier. Working tree was clean and
  its only commit was already present in `master`'s history — pure
  redundancy. 5.2 GB on disk (5.0 GB of that was just an uncleaned `target/`
  build directory), 390 files indexed.
- `/home/alaz/dev/core/R-AI-OS-worktrees/raios-tray-desktop-independent` — an
  orphaned worktree checkout with real uncommitted work (a GTK/AppIndicator3
  → portable Qt `QSystemTrayIcon` rewrite of `raios-tray.py`) that had never
  been merged. Recovered into `master` (commit `20e9743`) before deleting the
  worktree. 1.1 GB, 494 files indexed.
- `JUCE` and `ghostty` — pinned upstream reference clones (not authored
  workspace source) sitting directly under `dev/core` and `dev/tools`,
  contributing ~4,200 indexed files of third-party framework/terminal code.
  Added to `search::indexer::SKIP_DIRS` (commit `6a59937`) so they no longer
  get walked at all.

Fix sequence, in order: (1) recover any real uncommitted work out of stale
worktrees/clones before touching them, (2) delete the redundant/stale
directories, (3) add the reference clones to `SKIP_DIRS`, (4) force a full
BM25 + trigram reindex (`raios search "x" --reindex --dir ~/dev`, `raios
locate "x" --reindex --dir ~/dev`) so already-cached rows for now-gone/now-
skipped paths get pruned — an incremental (non-forced) reindex only compares
mtimes and won't evict rows for paths that vanished, (5) stop `aiosd` and run
`sqlite3 workspace.db "VACUUM;"` — SQLite does not shrink the file on
`DELETE` without an explicit `VACUUM`, restart `aiosd`.

Result: **4.4 GB → 2.0 GB**. The remaining ~2 GB is legitimate index content
for this workspace's real file count (~16,000 files across `bm25_postings` +
trigram tables) — not further junk. The soft cap above was raised from
500 MB to 2.5 GB accordingly: 500 MB was an initial guess made before any
real measurement existed, and was never achievable for a workspace this
size once actual data was in hand.

## PR review checklist

Before merging a change that adds a new hot table, a new frequent-insert
path, or a new large-blob column (e.g. Cortex embeddings, session
transcripts):

> **Does this change grow a hot table without a corresponding budget bump?**
> If it adds writes to `mem_items`, `cp_tasks`, `cp_agent_runs`,
> `cp_wrapper_events`, `cp_artifacts`, or `audit_log` — or introduces a new table that will
> accumulate rows over the life of the workspace — either (a) add/adjust a
> soft cap for it in this file and in `db_budget.rs`, or (b) explain in the
> PR why it's bounded by construction (e.g. a ring buffer like `cp_logs`,
> or a table that's rewritten in place rather than appended to).
