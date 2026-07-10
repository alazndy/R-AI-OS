# Cortex Daemon Residency — Design

**Date:** 2026-07-10
**Status:** Approved for planning
**Assignee:** Antigravity Kaira
**Sibling project (parallel, independent):** Trigram grep index (`2026-07-10-trigram-grep-design.md`, Codex Kaira). Coordination note at the bottom.

## Problem (verified in source today)

Every semantic search pays the full Cortex startup cost because nothing keeps the engine alive:

1. `Cortex::init()` → `VectorEngine::load()` (`crates/raios-runtime/src/cortex/store.rs:79-142`) reads ALL `cortex_chunks` rows (2000+) and calls `rebuild_hnsw()` **unconditionally** — measured today at ~3-6s per call in release mode, and it grows with the shared index. This dominates `raios search`'s total time now that BM25 is cached.
2. The daemon (`aiosd`) was supposed to solve this but doesn't: its `VectorSearch` TCP command (`crates/raios-runtime/src/daemon/handlers.rs`, the `"VectorSearch"` match arm) calls `Cortex::init()` **per request** — full model load + HNSW rebuild inside the daemon, every time.
3. The daemon's file-watcher worker (`crates/raios-runtime/src/daemon/cortex.rs`, `start_cortex_worker`) also calls `Cortex::init()` per changed file, indexes the file, then drops the instance — and since `Cortex::index_file` alone never calls `rebuild_hnsw()` (only the batch `index_workspace`/`index_project` paths do), those incremental writes update SQLite but the HNSW ever queried is whatever a later full `load()` rebuilds. The worker's per-event work is real cost with no query-path benefit.
4. `raios search` (CLI) doesn't talk to the daemon at all — it always does the full in-process init.

## Goals

- ONE long-lived `Cortex` instance living in `aiosd`, serving vector searches over the existing TCP line-JSON protocol in milliseconds (model loaded once, HNSW rebuilt only when dirty).
- `VectorSearch` daemon command served from that instance, with scope support (`search_scoped`, shipped earlier today).
- File-change events update the resident instance incrementally; HNSW rebuilt lazily (dirty flag, rebuild-before-next-search), not per event.
- `raios search` delegates the vector half to the daemon when reachable; **silent, complete fallback** to the current in-process path when not (daemon down, auth fail, timeout, malformed reply). BM25 half stays local (already cached and fast).
- Expected end state: warm `raios search` drops from ~4-6s to sub-second when the daemon is up; identical behavior to today when it's down.

## Non-Goals

- New protocol/port/IPC mechanism — reuse TCP 42069 line-JSON + `AUTH <token>` exactly as-is (client pattern to copy exists in `tools_workspace.rs::tool_get_validation_errors`).
- Changing embedding model, chunking, HNSW parameters, or the shared-SQLite storage.
- MCP-side delegation (`semantic_search` keeps its in-process path this round — one consumer migrated first, deliberately).
- Touching `search/indexer.rs`, `search/trigram.rs` or adding subcommands — those belong to the parallel trigram project.

## Design

### 1. Ownership: dedicated worker thread + channels (not a shared lock)

`fastembed::TextEmbedding`'s `Sync`-ness is NOT to be assumed — the safe, idiomatic design is a single OS thread that OWNS the `Cortex` and serves requests over a channel; nothing else ever touches the instance, so `Send`/`Sync` bounds never bind. (First implementation step must still verify what compiles — if `Cortex` turns out not even `Send`, construct it INSIDE the worker thread, which the design below already does.)

```rust
// daemon/cortex.rs — new request enum
pub enum CortexRequest {
    Search { query: String, top_k: usize, scope: Option<PathBuf>,
             reply: tokio::sync::oneshot::Sender<Vec<VectorResult>> },
    IndexFile { path: PathBuf },
    Reindex { scope: PathBuf, reply: tokio::sync::oneshot::Sender<usize> },
}
```

`start_cortex_worker` is reworked: spawn via `std::thread::spawn` (blocking work — model inference — must not sit on the tokio runtime); inside, `Cortex::init()` once (paying the load cost exactly once, at daemon startup), then loop on a `tokio::sync::mpsc::Receiver<CortexRequest>` (`blocking_recv`). State: `dirty: bool`. `IndexFile` → `cortex.index_file(&path)`, set `dirty = true` (NO rebuild). `Search` → if `dirty { cortex.rebuild_index(); dirty = false }` then `search_scoped` (or plain `search` when `scope` is None) and send on `reply` (ignore send error — client gone). `Reindex` → `index_project(&scope)`, `dirty = false` (batch path rebuilds internally), reply count. The existing broadcast-based file-watcher subscription forwards `FileChanged` events into the channel as `IndexFile` instead of doing its own per-event `Cortex::init()`.

The `mpsc::Sender<CortexRequest>` is stored where handlers can reach it — either a new field on `DaemonState` (precedent: it already holds `index: Option<ProjectIndex>`) or threaded to `handle_client_connection` alongside existing state; follow whichever wiring `handlers.rs` makes cleaner, but the sender must be cloneable per connection (mpsc senders are).

### 2. `VectorSearch` handler rework

Replace the per-request `Cortex::init()` in the `"VectorSearch"` arm with: build a `oneshot`, send `CortexRequest::Search` (include optional `"scope"` string arg from the JSON — absent means unscoped, preserving current behavior), `await` the reply with a timeout (e.g. 10s; on timeout reply an error event). Keep the existing BM25-from-state + RRF fusion + `VectorResults` response shape byte-compatible so the TUI (existing `VectorSearch` consumer) keeps working unchanged.

### 3. CLI delegation with fallback (`cmd_search` in `cli/search.rs`)

New private helper in `cli/search.rs`: `fn daemon_vector_search(query, top_k, scope) -> Option<Vec<VectorResult>>` — `TcpStream::connect_timeout("127.0.0.1:42069", 300ms)`; read `.session_token` (same path resolution as `tool_get_validation_errors`: `Config::config_file().parent().join(".session_token")`); send `AUTH`, send `{"command":"VectorSearch","query":…,"top_k":…,"scope":…}`; read reply lines with a short read timeout; parse `VectorResults` into `Vec<VectorResult>`. ANY failure at ANY step → `None`, no error printed (a debug-level eprintln behind `--json`-off is acceptable, but the fallback must be silent by default).

`cmd_search` flow becomes: try `daemon_vector_search` FIRST — on `Some(hits)`, skip `Cortex::init()` entirely (that's the whole point); on `None`, run today's exact in-process path unchanged. `--reindex` with a reachable daemon sends `Reindex` (new JSON command `"CortexReindex"` mapped to `CortexRequest::Reindex`) instead of local `index_project`.

Wire-format note: the daemon's reply serializes `VectorResult`-shaped JSON; the CLI must deserialize back into `cortex::store::VectorResult` (fields `path`, `start_line`, `text`, `score`) so `hybrid::fuse` consumes them identically to the in-process path. `VectorResult` already derives Serialize — add `Deserialize` if missing (check first).

## Testing

- Worker unit tests (in `daemon/cortex.rs`): dirty-flag logic — `IndexFile` then `Search` triggers exactly one rebuild; two `Search`es without changes rebuild zero times (instrument via a test-only counter or by observing timing is not acceptable — use a seam: extract the dirty-decision into a pure function or feature-gated counter). Channel round-trip: `Search` request returns results from a small TempDir-indexed corpus.
- Handler test: `VectorSearch` JSON in → `VectorResults` JSON out, shape-compatible with the pre-change format (assert exact keys).
- CLI fallback test: `daemon_vector_search` against a port nobody listens on → `None` fast (bounded by connect timeout); `cmd_search` still returns results via in-process path (this is effectively today's behavior test).
- Final verification (release build, real workspace): daemon up → warm `raios search` sub-second, results identical (same query, compare result paths against in-process run); daemon stopped → `raios search` still works at today's ~4-6s. `systemctl --user restart aiosd` after install; confirm the resident instance answers repeated searches without re-loading (daemon log prints the init line once).

## Risks & Coordination

- `Cortex`/`TextEmbedding` `Send` bound: worker-thread-owns-it design sidesteps `Sync`, and constructing inside the thread sidesteps `Send` for the instance itself — only `CortexRequest` needs `Send` (it is: PathBuf/String/oneshot). If compilation still fights, STOP and report rather than reaching for `unsafe`.
- Daemon restart required to pick up the binary — same install procedure used earlier today (unlink+recreate both `~/.cargo/bin/aiosd` and `~/.local/bin/aiosd`, then `systemctl --user restart aiosd`, then verify loaded inode via `lsof`).
- Stale-data window: daemon's resident index only knows what it indexed + file events since startup; a file changed while the daemon was down is invisible until reindex. Acceptable for v1 — `--reindex` is the escape hatch; note it in the final report.
- **Parallel-project conflict surface:** the trigram project (Codex) adds `search/trigram.rs`, `cli/grep.rs`, and appends to `cli/args.rs`, `cli/mod.rs`, `mcp/tools*.rs`. This project must NOT touch those. Shared-touch files: `cli/search.rs` (this project only), `cli/args.rs`/`cli/mod.rs` (NOT touched by this project — no new subcommand here). Whoever merges second rebases onto master and resolves any small overlap.
