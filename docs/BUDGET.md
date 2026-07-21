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
| `workspace.db` total file size | 500 MB | **warn** — `raios health` flags `OVER CAP`; nothing blocks yet |
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
