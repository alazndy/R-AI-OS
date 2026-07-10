# Cortex Daemon Residency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Design doc (read first — it contains today's verified findings about what's actually broken): `docs/superpowers/specs/2026-07-10-cortex-daemon-design.md`.

**Goal:** One long-lived `Cortex` inside `aiosd` serving vector searches in milliseconds over the existing TCP protocol; `raios search` delegates to it when reachable and falls back silently to today's in-process path when not.

**Architecture:** A dedicated OS thread owns the `Cortex` (sidestepping `Send`/`Sync` uncertainty on `fastembed::TextEmbedding`) behind a `tokio::sync::mpsc` channel with `oneshot` replies; the existing `VectorSearch` handler and the file-watcher worker both switch from per-call `Cortex::init()` to channel requests; the CLI gains a small TCP client with aggressive timeouts and total fallback.

**Tech Stack:** Rust, tokio (already the daemon runtime), no new crates.

## Global Constraints

- Repo `/home/alaz/dev/core/R-AI-OS`, branch from current `master`. NEW isolated git worktree.
- **Parallel project running simultaneously** (Codex: trigram grep). DO NOT touch: `search/indexer.rs`, `search/trigram.rs` (may not exist yet in your checkout — fine), `cli/grep.rs`, `cli/args.rs`, `cli/mod.rs`, `mcp/tools*.rs`. Your surface: `daemon/`, `cortex/` (additive only), `cli/search.rs`, `bin/aiosd.rs` if wiring demands. Whoever merges second rebases and resolves.
- Reuse TCP 127.0.0.1:42069 line-JSON + `AUTH <token>` protocol exactly. Client-side pattern to copy verbatim-style: `tools_workspace.rs::tool_get_validation_errors` (connect → AUTH from `.session_token` → JSON line → read reply lines).
- The `VectorResults` response shape must stay byte-compatible for existing consumers (TUI). Additive JSON fields only.
- NO `unsafe`. If `Send` bounds fight the worker design, STOP and report BLOCKED — do not force it.
- `cargo test --workspace` + `cargo check --workspace` clean after every task. Baseline 619 tests. Commit per task.

---

### Task 1: Compile-probe `Cortex`'s thread mobility + `VectorResult` Deserialize

**Files:**
- Modify: `crates/raios-runtime/src/cortex/store.rs` (derive only, if needed)

- [ ] **Step 1:** Check `VectorResult`'s derives: `grep -n "pub struct VectorResult" -B3 crates/raios-runtime/src/cortex/store.rs`. The CLI (Task 5) must deserialize daemon replies back into `VectorResult` — if `Deserialize` is missing from its derive list, add it (`serde::Deserialize`). `cargo check -p raios-runtime`.
- [ ] **Step 2:** Compile-probe thread mobility with a throwaway test in `daemon/cortex.rs`:

```rust
#[cfg(test)]
mod thread_probe {
    #[test]
    fn cortex_constructible_inside_a_thread() {
        // Only asserts the pattern compiles & runs: construct INSIDE the thread,
        // never move an instance across. This is the worker design's foundation.
        let h = std::thread::spawn(|| {
            let _ = crate::cortex::Cortex::init().map(|c| c.chunk_count());
        });
        h.join().unwrap();
    }
}
```

This needs no `Send` on `Cortex` at all (closure captures nothing). If even this fails to compile, STOP — report BLOCKED with the exact compiler error.

- [ ] **Step 3:** `git commit -m "chore(cortex): derive Deserialize on VectorResult, probe thread-local construction"` (only if changes were needed; otherwise fold into Task 2's commit).

---

### Task 2: `CortexWorker` — owned-instance request loop with dirty-flag rebuild

**Files:**
- Modify: `crates/raios-runtime/src/daemon/cortex.rs` (this file currently holds `start_cortex_worker`, which does per-event `Cortex::init()` — it gets rewritten; read it fully first)

**Interfaces:**
- Produces (Tasks 3-5 rely on these exact names):

```rust
pub enum CortexRequest {
    Search  { query: String, top_k: usize, scope: Option<PathBuf>,
              reply: tokio::sync::oneshot::Sender<Vec<crate::cortex::store::VectorResult>> },
    IndexFile { path: PathBuf },
    Reindex   { scope: PathBuf, reply: tokio::sync::oneshot::Sender<usize> },
}
pub fn spawn_cortex_worker(eager_indexing: bool) -> tokio::sync::mpsc::Sender<CortexRequest>
```

- [ ] **Step 1: Failing unit tests.** For testability WITHOUT threads, factor the logic into a plain struct and test it synchronously:

```rust
struct CortexWorkerState { cortex: crate::cortex::Cortex, dirty: bool, rebuilds: usize }

impl CortexWorkerState {
    fn handle(&mut self, req: CortexRequest) { /* Task Step 3 */ }
}
```

Tests (TempDir corpus, index a couple of files via `cortex.index_project` on a tiny workspace first):
- `index_then_search_rebuilds_exactly_once`: send `IndexFile`, then two `Search`es → `rebuilds == 1` and both searches return the indexed content.
- `search_without_changes_never_rebuilds`: fresh state (post-initial-index) → two `Search`es → `rebuilds == 0`.
- `reindex_clears_dirty`: `IndexFile` then `Reindex` then `Search` → `rebuilds` incremented only by the batch path (i.e. the counter tracks explicit `rebuild_index()` calls made by `handle`; `Reindex`'s internal batch rebuild doesn't need counting — assert `dirty == false` after `Reindex` and the follow-up `Search` does not bump the counter).

NOTE the embedding-mode caveat: unit tests run without the `cortex` feature only if the default features say so — this crate's default IS `cortex` (real fastembed). A real model download in unit tests is unacceptable. Check how existing cortex tests in `cortex/store.rs` handle this (they test `VectorEngine` directly, no embedder). Mirror that: give `CortexWorkerState` a constructor taking a pre-built `Cortex` OR refactor the dirty-flag decision into a tiny pure struct (`DirtyTracker { dirty, rebuilds }`) tested standalone, with `CortexWorkerState.handle` kept thin. **Prefer the pure-struct route if `Cortex::init()` in tests would download models — decide by reading how `Embedder::init()` behaves under `cfg(test)`, and document the choice in your report.**

- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: Implement** `handle`:

```rust
fn handle(&mut self, req: CortexRequest) {
    match req {
        CortexRequest::Search { query, top_k, scope, reply } => {
            if self.dirty {
                self.cortex.rebuild_index();
                self.rebuilds += 1;
                self.dirty = false;
            }
            let hits = match scope {
                Some(dir) => self.cortex.search_scoped(&query, top_k, &dir).unwrap_or_default(),
                None => self.cortex.search(&query, top_k).unwrap_or_default(),
            };
            let _ = reply.send(hits);
        }
        CortexRequest::IndexFile { path } => {
            if let Ok(true) = self.cortex.index_file(&path) {
                self.dirty = true;
            }
        }
        CortexRequest::Reindex { scope, reply } => {
            let n = self.cortex.index_project(&scope).unwrap_or(0);
            self.dirty = false; // batch path rebuilds internally
            let _ = reply.send(n);
        }
    }
}
```

(`Cortex::rebuild_index()` exists — added during the memory-port work; confirm with `grep -n "pub fn rebuild_index" crates/raios-runtime/src/cortex/mod.rs`. `index_file` returns `Result<bool>` — only `true` means content actually (re)indexed, so only that sets `dirty`.)

`spawn_cortex_worker`: create `mpsc::channel(64)`; `std::thread::spawn(move || { ... })` constructing `Cortex::init()` INSIDE; optional eager `index_workspace` (preserve the current `eager_indexing` behavior + its log lines); then `while let Some(req) = rx.blocking_recv() { state.handle(req) }`. Also move the existing broadcast `FileChanged` subscription logic: the async side (wherever `start_cortex_worker` was subscribed — find its caller: `grep -rn "start_cortex_worker" crates/`) now forwards events as `IndexFile` sends into this channel instead. Keep `is_indexable()` as the filter, unchanged.

- [ ] **Step 4:** tests PASS; `cargo check --workspace` clean (old `start_cortex_worker` callers updated — find them all, likely `daemon/mod.rs` or `kernel`).
- [ ] **Step 5:** `git commit -m "feat(daemon): resident Cortex worker thread with lazy HNSW rebuild"`

---

### Task 3: Wire the sender through the daemon

**Files:**
- Modify: wherever `start_cortex_worker` was spawned (locate: `grep -rn "start_cortex_worker\|cortex_worker" crates/ --include="*.rs"`) + `daemon/handlers.rs`'s connection setup so each client handler can clone the `Sender<CortexRequest>`.

- [ ] **Step 1:** Map the wiring first — run the grep above plus `grep -n "handle_client_connection\|ClientHandle" crates/raios-runtime/src/daemon/server.rs crates/raios-runtime/src/daemon/handlers.rs | head -20`. Two viable homes for the sender: (a) a field on `ClientHandle`/handler args, (b) a field on `DaemonState` (`pub cortex_tx: Option<mpsc::Sender<CortexRequest>>` — precedent: state already carries `index: Option<ProjectIndex>`). Choose (b) unless the handler plumbing makes (a) obviously cleaner — state is already threaded everywhere the handler goes. Set it in `aiosd.rs`/kernel startup right after `spawn_cortex_worker(...)`.
- [ ] **Step 2:** Implement; `cargo check --workspace` clean. No behavior change yet (nothing reads the field until Task 4) — commit: `git commit -m "feat(daemon): thread resident-cortex sender through daemon state"`

---

### Task 4: Rework `VectorSearch` + add `CortexReindex` daemon commands

**Files:**
- Modify: `crates/raios-runtime/src/daemon/handlers.rs` (the `"VectorSearch"` match arm; add `"CortexReindex"` arm next to it)

- [ ] **Step 1:** Read the current arm in full (quoted in the design doc; re-read live). Replace its per-request `Cortex::init()` block:

```rust
"VectorSearch" => {
    if let Some(query) = v["query"].as_str() {
        let top_k = v["top_k"].as_u64().unwrap_or(10) as usize;
        let scope = v["scope"].as_str().map(std::path::PathBuf::from);
        let vector_hits = {
            let tx_opt = { state_for_client.read().await.cortex_tx.clone() };
            match tx_opt {
                Some(tx) => {
                    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                    let sent = tx.send(crate::daemon::cortex::CortexRequest::Search {
                        query: query.to_string(), top_k, scope, reply: reply_tx,
                    }).await.is_ok();
                    if sent {
                        tokio::time::timeout(std::time::Duration::from_secs(10), reply_rx)
                            .await.ok().and_then(|r| r.ok()).unwrap_or_default()
                    } else { vec![] }
                }
                None => vec![],
            }
        };
        // ... existing BM25-from-state + fuse + VectorResults response: UNCHANGED below this line
```

Keep everything from `let bm25_hits = ...` down identical (response shape compatibility). `"CortexReindex"` arm: parse `"scope"` (required), send `Reindex`, await with the same timeout pattern, reply `{"event":"CortexReindexed","indexed":n}`.

- [ ] **Step 2:** Existing handler tests in this file (there are several `mod ..._tests`) must still pass: `cargo test -p raios-runtime daemon` — plus add one shape test: a `VectorSearch` request against a state with `cortex_tx: None` still yields a well-formed `VectorResults` response (empty vector hits, BM25-only fusion) — this doubles as the no-worker degradation test.
- [ ] **Step 3:** `git commit -m "feat(daemon): VectorSearch served by resident cortex, add CortexReindex"`

---

### Task 5: CLI delegation with silent fallback

**Files:**
- Modify: `crates/raios-surface-cli/src/cli/search.rs` ONLY (parallel-project constraint: do not touch args.rs/mod.rs — no new flags; delegation is automatic).

- [ ] **Step 1: Failing test** (inline `#[cfg(test)]` in `search.rs` — first test module in this file, fine): `daemon_vector_search_returns_none_when_unreachable`: call the helper against port `1` (nothing listens) → `None`, and elapsed < 1s (proves the connect timeout bounds the fallback cost).
- [ ] **Step 2: Implement the client helper** (model the AUTH/read flow on `tool_get_validation_errors`, but with timeouts everywhere):

```rust
fn daemon_vector_search(query: &str, top_k: usize, scope: &Path)
    -> Option<Vec<raios_runtime::cortex::store::VectorResult>> {
    use std::io::{BufRead, BufReader, Write};
    let addr: std::net::SocketAddr = "127.0.0.1:42069".parse().ok()?;
    let stream = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(15))).ok()?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2))).ok()?;
    let mut writer = stream.try_clone().ok()?;
    let token_path = raios_core::config::Config::config_file()
        .parent().map(|p| p.to_path_buf()).unwrap_or_default().join(".session_token");
    let token = std::fs::read_to_string(token_path).ok()?;
    writer.write_all(format!("AUTH {}\n", token.trim()).as_bytes()).ok()?;
    let req = serde_json::json!({
        "command": "VectorSearch", "query": query, "top_k": top_k,
        "scope": scope.to_string_lossy(),
    });
    writer.write_all(format!("{req}\n").as_bytes()).ok()?;
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line.ok()?;
        let v: serde_json::Value = serde_json::from_str(&line).ok()?;
        if v["event"] == "VectorResults" {
            // Daemon emits fused results; we need the raw vector hits. See Step 3.
            return serde_json::from_value(v["vector_hits"].clone()).ok();
        }
    }
    None
}
```

- [ ] **Step 3: Resolve the wire-format decision this exposes.** The existing `VectorResults` response carries FUSED results (post-RRF, with BM25 mixed in) — not the raw `VectorResult` list `cmd_search` needs (the CLI does its own fusion with its own freshly-cached local BM25). Do NOT repurpose the fused list. Instead, in Task 4's handler, ADD an additive field to the same response line: `"vector_hits": <raw Vec<VectorResult> serialized>` alongside the existing `"results"` — additive JSON keeps the TUI compatible while giving the CLI what it needs. (Go back and amend Task 4's response construction accordingly; this ordering is deliberate — the need only becomes concrete here, and the plan says so rather than pretending it was obvious upfront.)
- [ ] **Step 4: Integrate into `cmd_search`:** at the top of the vector path — `let daemon_hits = daemon_vector_search(query, top_k, scope);` — on `Some(hits)`: skip `Cortex::init()`/indexing/`search_scoped` entirely, use `hits` as `vector_hits`, and route `--reindex` through a similar tiny client sending `CortexReindex` (on `None` reindex response, fall through to local reindex). On `None`: today's exact path, untouched. The `needs_index`/first-run printing logic runs ONLY on the fallback path.
- [ ] **Step 5:** unit test PASSES; `cargo test --workspace` clean. Manual: with `systemctl --user stop aiosd` → `./target/release/raios search "memory layer" --top-k 5` behaves exactly like today (timing ~4-6s); this is the fallback proof. (Daemon-up verification happens in Task 6 after install, since the RUNNING daemon is the old binary.)
- [ ] **Step 6:** `git commit -m "feat(cli): raios search delegates vector search to resident daemon with silent fallback"`

---

### Task 6: Install, restart, end-to-end verification

- [ ] **Step 1:** `cargo test --workspace` (record count) + `cargo build --release --workspace`.
- [ ] **Step 2:** Install BOTH binaries with the unlink-first procedure (a running binary gives "Text file busy" on plain `cp` — this exact issue occurred today):

```bash
rm ~/.cargo/bin/aiosd && cp target/release/aiosd ~/.cargo/bin/aiosd
rm ~/.local/bin/aiosd && cp target/release/aiosd ~/.local/bin/aiosd
rm ~/.local/bin/raios && cp target/release/raios ~/.local/bin/raios
rm ~/.cargo/bin/raios && cp target/release/raios ~/.cargo/bin/raios
systemctl --user restart aiosd
lsof -p $(systemctl --user show -p MainPID --value aiosd) | grep aiosd$   # verify new inode loaded
```

- [ ] **Step 3:** End-to-end timing, the whole point of this project:

```bash
cd /home/alaz/dev/core/R-AI-OS
time raios search "memory layer" --top-k 5    # daemon warm → expect SUB-SECOND
time raios search "memory layer" --top-k 5    # repeat → stable sub-second
systemctl --user stop aiosd
time raios search "memory layer" --top-k 5    # fallback → ~4-6s, identical results
systemctl --user start aiosd
```

Record all three timings. Compare result paths between daemon-served and fallback runs for the same query — they must be substantively identical (exact score ordering may differ slightly if the daemon's index is fresher; investigate any missing/extra file rather than hand-waving).

- [ ] **Step 4:** Hand off back to claude-kaira via `raios handoff` with: final test count, the three timings, the identical-results check outcome, and any deviations from this plan. Do not merge/push without that handoff.
