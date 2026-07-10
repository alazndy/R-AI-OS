# BM25 Cache Wiring — Design

**Date:** 2026-07-10
**Status:** Approved for planning
**Scope:** Wire the existing (tested, unused) `ProjectIndex::load_or_build()` mtime-cache into the two real search entry points, fixing the latent defects that make it unsafe to wire as-is. First of four search-infrastructure sub-projects (this → stale-row cleanup → trigram index → Cortex-in-daemon).

## Problem

1. Both real BM25 call sites — `cmd_search` (`crates/raios-surface-cli/src/cli/search.rs`) and `tool_semantic_search` (`crates/raios-surface-mcp/src/mcp/tools_workspace.rs`) — call `ProjectIndex::build(scope)`, which re-walks the tree, re-reads every matching file, and re-tokenizes everything on **every query**. A cached variant, `load_or_build(root, db_path)`, exists in `crates/raios-runtime/src/search/indexer.rs` with three passing tests, but nothing calls it.
2. `load_or_build` cannot be wired as-is. It has two latent defects:
   - **Cross-scope cache eviction:** `load_cached_bm25_files` reads ALL rows in `bm25_files` (no scope filter); every cached file not found under the current `root` is treated as stale and **deleted**. With the shared `workspace.db`, searching project A then project B evicts A's entire cache — an agent alternating between projects gets near-zero cache benefit and constant rebuild churn.
   - **Quadratic, unbatched cold write:** for each newly indexed file, the write loop iterates the ENTIRE `inverted` map (including warm postings loaded from cache) to find that file's postings — O(new_files × total_postings) — and issues one autocommit `INSERT` per posting. A cold build over ~230 files would be dramatically slower than today's uncached `build()`, i.e. naive wiring would cause a first-run regression.
3. Path-form inconsistency: `raios search --dir .` walks relative paths (`./docs/…`) while the cwd default produces absolute paths. Cached under different key forms, the same file dodges its own cache entry and confuses scope filtering.

## Goals

- `raios search` / `semantic_search` reuse the BM25 index across invocations; only changed/new/deleted files (by mtime) are re-tokenized.
- Multiple projects share `workspace.db` without evicting each other's cache.
- `--reindex` forces a full BM25 rebuild for the current scope (matching its existing Cortex semantics).
- Cold-cache first run is no slower than today's `build()` path.

## Non-Goals

- Trigram/substring indexing (separate sub-project).
- Moving Cortex/HNSW into the `aiosd` daemon (separate sub-project).
- Cleaning legacy stale rows (3 old `cortex_chunks` rows under `target/`, any legacy relative-path `bm25_files` rows) — separate data-hygiene sub-project; noted in Risks.

## Design

Five surgical changes, no new files, no new dependencies.

### 1. Shared DB path: make `default_db_path()` public

`crates/raios-runtime/src/cortex/store.rs`'s private `default_db_path()` (→ `~/.config/raios/workspace.db`) becomes `pub`. Both call sites use it for `load_or_build`'s `db_path` argument. One source of truth; no duplicated path resolution.

### 2. Scope-aware cache read (fixes cross-scope eviction)

Inside `load_or_build`, immediately after `load_cached_bm25_files(&conn)` returns, narrow the map in Rust:

```rust
cached.retain(|path, _| Path::new(path).starts_with(&root));
```

Component-aware `Path::starts_with` — never a string prefix and never SQL `LIKE` (avoids `%`/`_` escaping pitfalls and the `R-AI-OS` vs `R-AI-OS-fork` sibling-prefix bug class already fixed in Cortex's `search_scoped`). Out-of-scope rows are simply never seen, so they can no longer be mistaken for stale and deleted. Stale detection within scope is unchanged: a cached in-scope file missing from the fresh walk is still correctly evicted.

### 3. Canonicalize `root` once at `load_or_build` entry

```rust
let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
```

`WalkDir` yields paths prefixed by the root as passed, so one canonicalization makes every walked path canonical — no per-file syscalls. Relative and absolute invocations of the same directory now share one cache keyspace.

### 4. Cold-write performance fix (fixes quadratic/unbatched writes)

- `index_file` (private; one other caller, `build()`, ignores the return) returns the postings it created: `fn index_file(&mut self, path: PathBuf, content: &str) -> Vec<(String, usize, String)>` — `(token, line_no, snippet)` for this file only. The insert loop in `load_or_build` uses that return value directly instead of scanning `self.inverted`, making writes O(postings-of-this-file).
- The entire write phase (stale `DELETE`s + all `INSERT`s) is wrapped in a single `conn.unchecked_transaction()` … `tx.commit()` — the codebase's established pattern (`mem.rs`, `cortex/store.rs`, `wf_handoff.rs`).

### 5. `force` parameter (wires `--reindex`)

`load_or_build(root, db_path, force: bool)`. When `force` is true, skip the warm/stale computation entirely — every filesystem file is treated as new. `INSERT OR REPLACE` on `bm25_files(path UNIQUE)` plus `ON DELETE CASCADE` on `bm25_postings` (with `PRAGMA foreign_keys=ON`, already set in `open_bm25_db`) refreshes rows cleanly; no bespoke delete pass needed.

### Call-site wiring

- `cmd_search`: `ProjectIndex::build(scope)` → `ProjectIndex::load_or_build(scope, &default_db_path(), reindex)`. So `--reindex` now forces both engines, consistent with the chosen semantics.
- `tool_semantic_search`: → `load_or_build(&scope, &default_db_path(), false)` (MCP surface has no reindex flag today; not adding one — YAGNI).
- Error handling unchanged: both sites already `match` the `Result` and degrade to an empty BM25 hit list; `hybrid::fuse` then runs vector-only.

## Testing

All in `indexer.rs`'s existing inline test module, following its `TempDir` + isolated `test.db` convention (never the real shared DB):

- Existing three tests (`load_or_build_creates_index`, `second_load_uses_cache`, `modified_file_triggers_reindex`) updated for the `force` parameter (pass `false`).
- **Scope isolation (the eviction bug):** create workspaces A and B in one temp dir sharing one `test.db`; `load_or_build(A, db, false)`, then `load_or_build(B, db, false)`, then assert via direct SQL that A's `bm25_files` rows still exist, and that a fresh `load_or_build(A, db, false)` warm-loads (doc_count unchanged, results still returned).
- **Force rebuild:** two calls with `force: true` on an unchanged workspace both succeed; `bm25_files` row count stays constant (REPLACE, not duplicate); search results unchanged.
- **Per-file postings return:** after indexing a two-file workspace cold, assert `bm25_postings` row count equals the sum of per-file token counts (guards the change-4 refactor against writing warm/foreign postings).
- Full gate: `cargo test --workspace` and `cargo check --workspace` clean; manual release-build timing of `raios search` before/after (baseline: ~3.9s release; expect warm runs to drop by roughly the BM25 walk+tokenize share).

## Risks & Notes

- Legacy cache rows in non-canonical (relative) path form and the 3 stale `cortex_chunks` rows under `target/` remain in the shared DB, unreachable by scope filtering — harmless but untidy; handled by the separate data-hygiene sub-project.
- `bm25_postings` stores one row per token occurrence; the shared DB will grow with active use across projects. Acceptable at current scale; the trigram sub-project supersedes this table's design anyway.
- Concurrent `raios` processes may open `workspace.db` simultaneously; WAL mode (already set) plus the new single-transaction write keeps this safe — worst case a writer briefly blocks on the lock.
