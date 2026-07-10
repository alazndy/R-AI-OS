# Search Timing Root-Cause Diagnostic Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:systematic-debugging for this entire task. This is Phase 1 (Root Cause Investigation) of that skill — you MUST gather the evidence below and form exactly ONE hypothesis before writing any fix. Do NOT skip to Phase 4 (Implementation). If the evidence points to more than one plausible cause, report back and ask before picking one to fix.

**Goal:** Find why `raios search`/`semantic_search` went from ~12s (old, unscoped, whole-workspace behavior) to 100+ seconds (new, project-scoped behavior) even on a warm Cortex cache, and fix the actual root cause — not a guess.

**Architecture:** Add temporary `eprintln!` timing instrumentation at every stage boundary inside `cmd_search`, run it once under controlled conditions, capture the breakdown, then interpret it against the decision tree in this document.

**Tech Stack:** Rust, `std::time::Instant`. No new dependencies.

## Global Constraints

- Repo: `/home/alaz/dev/core/R-AI-OS`, branch `master`, currently at commit `f5d73bd` (a real, tested, already-merged fix: `raios search`/`semantic_search` now default to project-scope instead of the whole `~/dev` workspace, and a cross-project vector-search data leak was fixed via `Cortex::search_scoped()`). This diagnostic plan builds ON TOP of that commit — do not revert or second-guess it, its own tests already pass (`cortex::tests::scoped_filter_*`, 2 tests, verified).
- Set up a NEW isolated git worktree before touching anything (`superpowers:using-git-worktrees` consent flow, or `git worktree add` if no native tool is available in your harness).
- This machine runs multiple concurrent agent/daemon processes that share both CPU and a single SQLite file (`~/.config/raios/workspace.db`): an `aiosd` systemd service, and potentially other `raios run <agent>` sessions. **This is a real environmental variable, not noise to explain away** — Step 1 below captures it explicitly so you know your baseline.
- Do not make more than ONE code change before re-measuring. Stacking fixes was explicitly what the prior investigation (this same session, before handoff) was warned against by its own process.
- `cargo test --workspace` and `cargo check --workspace` must both stay clean throughout — this repo currently has 615 passing tests, 0 failures (430 raios-core, 170 raios-runtime, 9 raios-surface-cli, 2 raios-surface-mcp, 4 raios-surface-tui — confirm this exact baseline yourself in Step 0, don't trust this number blindly, it may have drifted).

---

## Confirmed Facts (already gathered, do not re-derive)

1. **Before** the project-scoping fix (commit `f5d73bd`): `raios search "memory layer" --top-k 5` ran `Cortex::index_workspace(dev_ops_path)` (walks `~/dev`, capped at `MAX_FILES_PER_INDEX = 5_000` files, ~5700 real files exist under `~/dev` matching the indexed extensions — meaning the cap likely truncated the walk before ever reaching every project) + `ProjectIndex::build(dev_ops_path)` (BM25, no cap, no cache — walks and re-tokenizes every matching file on every call, confirmed by reading `crates/raios-runtime/src/search/indexer.rs`'s `build()` — there IS a cached, mtime-aware `load_or_build()` in the same file with passing tests, but **it is never called by any real entry point**, confirmed by `grep -rn "ProjectIndex::build\|ProjectIndex::load_or_build" crates/`). Two consecutive runs: 12.37s and 11.61s wall-clock, ~26s CPU-time both times, ~230% CPU.
2. **After** the project-scoping fix: same query, scoped to `cwd` (`/home/alaz/dev/core/R-AI-OS`, ~230-ish source files by Sigmap's own earlier count) via `Cortex::index_project(scope)` (no file cap, direct `WalkDir`) + `ProjectIndex::build(scope)` (same uncached BM25 rebuild as before, just a smaller root). First attempt: killed by a 90s timeout with **zero stdout output** even though `println!("Cortex: First run — indexing {}...", scope.display())` should print before any expensive work starts (Rust's stdout is fully buffered when not connected to a TTY — a Bash-tool-captured run counts as not-a-TTY — so "no output before timeout" does NOT prove nothing happened in that time; it only proves the buffer was never flushed, which happens on normal process exit or an explicit flush). Second attempt (no timeout, let it finish): 109.44s wall, 701.90s CPU, 641% CPU, ended with `"Cortex: 2059 chunks loaded from cache"` — meaning `needs_index` was `false` for THIS run (Cortex's on-disk index already had chunks from the previous, differently-scoped runs — Cortex's cache is keyed by absolute file path + mtime in a **shared** SQLite table, not scoped by which root a given call used to discover files, so results accumulate across differently-scoped invocations).
3. After also fixing the cross-project vector leak (`search_scoped`, commit `f5d73bd`): same query, same warm-cache "2059 chunks loaded from cache" path, **118.12s wall, 725.26s CPU, 614% CPU**. Results were now correctly scoped to `/home/alaz/dev/core/R-AI-OS` files only (the leak fix works), but included 2 stale `target/.../fingerprint/*.json` entries — these are pre-existing rows in the shared `cortex_chunks` table from some earlier indexing run (only 3 such rows exist total, confirmed via `sqlite3 ~/.config/raios/workspace.db "SELECT COUNT(*) FROM cortex_chunks WHERE path LIKE '%/target/%'"`), NOT newly created by the current code (current `SKIP_DIRS` in `crates/raios-runtime/src/cortex/mod.rs` correctly includes `"target"`, confirmed by reading the constant). This is a separate, minor data-hygiene issue — **do not spend time on it in this diagnostic pass** unless it turns out to be relevant to the timing (it almost certainly isn't: 3 stale rows can't explain 100+ seconds).
4. At the time of measurement #3, this machine had load average **4.04 / 4.77 / 3.47** (1/5/15-min) on an **8-core** machine, and these processes were confirmed running concurrently via `pgrep -fa "cargo|rustc|raios"`: `aiosd` (systemd daemon), a `raios run claude` session, a `raios run codex` session (mid-handoff, the exact plan-execution session that did the project-scoping fix's sibling work earlier today), a `raios run opencode` session, and `raios-tray.py`. **This was NOT re-measured under quieter conditions** — that is exactly what Step 1 below must do.
5. **Warm-cache runs were NOT meaningfully faster than the first run** in either the pre-fix or post-fix measurements (12.37s → 11.61s is barely a difference; both post-fix runs that hit "chunks loaded from cache" were still 100+s) — this rules out "Cortex embedding of new files" as the sole explanation for the post-fix slowdown, since a warm run by definition skips `index_project`/`index_workspace` entirely (`needs_index = reindex || cortex.chunk_count() == 0` was `false` both post-fix times). Something in the code path that runs UNCONDITIONALLY (`Cortex::init()`, `cortex.search_scoped()`, `ProjectIndex::build(scope)`, `idx.search(query)`, or `hybrid::fuse()`) is the dominant cost, or it's pure environmental contention. Step 1 distinguishes between these.

## Open Questions (what this plan must answer)

- **Q1:** Is the 100+s cost algorithmic (something in raios's own code got slower between the two measurement points) or environmental (system contention from concurrent processes, which happened to be worse during the later measurements)?
- **Q2:** If algorithmic, which specific stage dominates: `Cortex::init()` (ONNX model load), `ProjectIndex::build(scope)` (uncached BM25 walk+tokenize), `cortex.search_scoped()` (embed query + HNSW query + in-memory filter), or something inside `hybrid::fuse()`?
- **Q3:** Does `Cortex::init()` → `VectorEngine::load()` block on SQLite access to the shared `~/.config/raios/workspace.db`, and if so, is that lock contended by the other concurrently-running `raios run <agent>` processes seen in Fact 4?

---

## Step 0: Confirm the test baseline

```bash
cd /home/alaz/dev/core/R-AI-OS
git log --oneline -1   # expect: f5d73bd fix(search): scope raios search / semantic_search to the current project
cargo test --workspace 2>&1 | grep -E "^test result:|FAILED"
```//
Record the exact pass counts per crate. If anything fails here, STOP — that's a different, pre-existing problem, report it before proceeding with this diagnostic.

## Step 1: Capture the environmental baseline

Before touching any code, run this exactly as given and record the full output — this answers Q1 partially by telling you how loaded the machine is RIGHT NOW, not at the time of the earlier (possibly unrepresentative) measurements:

```bash
echo "=== loadavg ==="; cat /proc/loadavg
echo "=== cores ==="; nproc
echo "=== concurrent raios/cargo/rustc processes ==="; pgrep -fa "cargo|rustc|raios" | grep -v "pgrep\|zsh -c"
echo "=== is anything holding a lock on the shared DB right now? ==="; lsof ~/.config/raios/workspace.db 2>/dev/null || fuser ~/.config/raios/workspace.db 2>/dev/null || echo "(lsof/fuser unavailable or no lock holder found)"
```

If load average and the concurrent-process list look similar to Fact 4 above (load ~4+ on 8 cores, 3+ other `raios run` processes alive), you are measuring under the SAME contention as before — proceed to Step 2 anyway (the instrumentation will still tell you which stage is slow even under load; if `Cortex::init()` alone eats 100s under contention, that's a real, actionable finding — "don't run cold `raios search` while N other agent sessions are active" or "make init faster/cache the model" are both legitimate fixes). If the machine is now quiet (load < 1-2, no other `raios run` processes), note that explicitly — a clean run under different conditions is itself useful signal for isolating Q1.

## Step 2: Add stage-by-stage timing instrumentation

**File:** `crates/raios-surface-cli/src/cli/search.rs`

Read the current file first — it should match exactly what's quoted below (this plan was written against the file as it exists in commit `f5d73bd`; if it has drifted, stop and reconcile before editing):

```rust
pub(super) fn cmd_search(query: &str, top_k: usize, reindex: bool, scope: &Path, json: bool) {
    let mut cortex = match raios_runtime::cortex::Cortex::init() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cortex init failed: {e:?}");
            return;
        }
    };
    let needs_index = reindex || cortex.chunk_count() == 0;

    if needs_index {
        if !json {
            if reindex {
                println!("Cortex: Re-indexing {} (forced)...", scope.display());
            } else {
                println!("Cortex: First run — indexing {}...", scope.display());
            }
        }
        let indexed = cortex.index_project(scope).unwrap_or(0);
        if !json {
            println!("Indexed {} chunks. Searching...\n", indexed);
        }
    } else if !json {
        println!(
            "Cortex: {} chunks loaded from cache. Searching...\n",
            cortex.chunk_count()
        );
    }

    let vector_hits = cortex.search_scoped(query, top_k, scope).unwrap_or_default();
    let bm25_hits = match raios_runtime::indexer::ProjectIndex::build(scope) {
        Ok(idx) => idx.search(query),
        Err(e) => {
            eprintln!("Index build failed: {e}");
            vec![]
        }
    };
    let fused = raios_runtime::hybrid_search::fuse(bm25_hits, vector_hits, top_k);
```

Replace it with this instrumented version — every `eprintln!` here writes to STDERR (unbuffered by default, so you'll see progress live even when stdout is fully buffered/redirected) and uses `std::time::Instant` to report elapsed milliseconds for exactly one stage each:

```rust
pub(super) fn cmd_search(query: &str, top_k: usize, reindex: bool, scope: &Path, json: bool) {
    let t_total = std::time::Instant::now();

    let t = std::time::Instant::now();
    let mut cortex = match raios_runtime::cortex::Cortex::init() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cortex init failed: {e:?}");
            return;
        }
    };
    eprintln!("[trace] Cortex::init(): {:?}", t.elapsed());

    let needs_index = reindex || cortex.chunk_count() == 0;
    eprintln!("[trace] needs_index={needs_index} (chunk_count={})", cortex.chunk_count());

    if needs_index {
        if !json {
            if reindex {
                println!("Cortex: Re-indexing {} (forced)...", scope.display());
            } else {
                println!("Cortex: First run — indexing {}...", scope.display());
            }
        }
        let t = std::time::Instant::now();
        let indexed = cortex.index_project(scope).unwrap_or(0);
        eprintln!("[trace] cortex.index_project(): {:?} ({indexed} chunks)", t.elapsed());
        if !json {
            println!("Indexed {} chunks. Searching...\n", indexed);
        }
    } else if !json {
        println!(
            "Cortex: {} chunks loaded from cache. Searching...\n",
            cortex.chunk_count()
        );
    }

    let t = std::time::Instant::now();
    let vector_hits = cortex.search_scoped(query, top_k, scope).unwrap_or_default();
    eprintln!("[trace] cortex.search_scoped(): {:?} ({} hits)", t.elapsed(), vector_hits.len());

    let t = std::time::Instant::now();
    let bm25_index = raios_runtime::indexer::ProjectIndex::build(scope);
    eprintln!("[trace] ProjectIndex::build(): {:?}", t.elapsed());

    let t = std::time::Instant::now();
    let bm25_hits = match bm25_index {
        Ok(idx) => idx.search(query),
        Err(e) => {
            eprintln!("Index build failed: {e}");
            vec![]
        }
    };
    eprintln!("[trace] idx.search(): {:?} ({} hits)", t.elapsed(), bm25_hits.len());

    let t = std::time::Instant::now();
    let fused = raios_runtime::hybrid_search::fuse(bm25_hits, vector_hits, top_k);
    eprintln!("[trace] hybrid::fuse(): {:?}", t.elapsed());

    eprintln!("[trace] TOTAL: {:?}", t_total.elapsed());
```

(The rest of the function — the `if json { ... }` block and the human-readable printing loop below it — is unchanged; only the block above it is replaced.)

Also instrument `Cortex::init()` itself, since Fact 5 says warm runs aren't faster and `init()` runs unconditionally every single call — split its two sub-steps in `crates/raios-runtime/src/cortex/mod.rs`:

```rust
pub fn init() -> Result<Self> {
    let embedder = Embedder::init()?;
    let engine = VectorEngine::load();
    Ok(Self { embedder, engine })
}
```

Replace with:

```rust
pub fn init() -> Result<Self> {
    let t = std::time::Instant::now();
    let embedder = Embedder::init()?;
    eprintln!("[trace]   Embedder::init(): {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let engine = VectorEngine::load();
    eprintln!("[trace]   VectorEngine::load(): {:?}", t.elapsed());
    Ok(Self { embedder, engine })
}
```

## Step 3: Build and run once

```bash
cargo build -p raios-surface-cli 2>&1 | tail -5
```

Immediately before running, re-capture Step 1's environmental snapshot (load may have changed):

```bash
cat /proc/loadavg
```

Then run exactly once, capturing everything (stdout AND stderr, since the trace lines go to stderr):

```bash
time ./target/debug/raios search "memory layer" --top-k 5 > /tmp/trace-run.log 2>&1
cat /tmp/trace-run.log
```

If it hangs past 5 minutes, let it keep running in the background (per this project's convention, use a backgrounded shell command rather than `timeout`, which — per Fact 2 — can kill the process before its buffered output is ever flushed and destroy the exact evidence this step exists to capture). Do not kill it. Do not start a second concurrent run.

## Step 4: Interpret the trace — decision tree

Look at which single `[trace]` line's duration is closest to the `TOTAL`. Exactly one of these should dominate:

### Branch A: `Embedder::init()` or `VectorEngine::load()` dominates
→ This means EVERY invocation of `raios search`/`semantic_search` pays this cost fresh, regardless of scope or cache state, because raios is a short-lived CLI process (no persistent server holding the model in memory between calls). This is likely the real root cause, and it existed before the project-scoping fix too — Fact 1's "warm run barely faster than cold" for the OLD code is consistent with this, since `Cortex::init()` ran unconditionally there too. If this branch is confirmed:
- Do NOT try to make ONNX model loading itself faster (out of scope, that's `fastembed`'s problem, not raios's).
- The legitimate fix is **caching the loaded model/engine across invocations** — either via the `aiosd` daemon (it's already running per Fact 4; check `crates/raios-runtime/src/daemon/` for whether it already holds a long-lived `Cortex` instance and whether `raios search` could delegate to it over the daemon's existing IPC instead of re-`init()`-ing in-process every time) or by explicitly documenting this as a known cost and moving on. **Do not implement a caching layer speculatively — first check whether `aiosd` already solves this and `raios search` simply isn't using it.**

### Branch B: `ProjectIndex::build()` dominates
→ This confirms the BM25-no-cache issue flagged during investigation (Fact 1: `load_or_build()` exists, tested, unused). If this branch is confirmed:
- Write a failing test first (per systematic-debugging Phase 4 and `superpowers:test-driven-development`): a benchmark-style test isn't appropriate for `cargo test`, so instead write a test that asserts `ProjectIndex::build()` and a subsequent `ProjectIndex::load_or_build()` call against the SAME root produce equivalent `search()` results for a fixed query (proving the cached path is behaviorally equivalent before you swap to it) — model it on the existing `load_or_build_creates_index`/`second_load_uses_cache`/`modified_file_triggers_reindex` tests already in `crates/raios-runtime/src/search/indexer.rs`, which you should read in full first.
- Fix: change the two real call sites (`crates/raios-surface-cli/src/cli/search.rs`, `crates/raios-surface-mcp/src/mcp/tools_workspace.rs`) from `ProjectIndex::build(scope)` to `ProjectIndex::load_or_build(scope, &db_path)` (check its exact signature in `indexer.rs` — it takes a second `db_path: &Path` argument; use the same `~/.config/raios/workspace.db` path Cortex itself uses, check `VectorEngine::load()`/`load_from()` in `crates/raios-runtime/src/cortex/store.rs` for how that path is currently resolved, and reuse the same resolution logic rather than hardcoding it twice).

### Branch C: `cortex.search_scoped()` dominates
→ Unexpected — this is supposed to be a single query embedding + an in-memory HNSW query + a `Vec` filter/sort/truncate over at most `top_k * 10` candidates, all sub-second by design. If this branch is confirmed:
- Check whether `embed_one()` → `embed_batch()`'s adaptive throttling (`crates/raios-runtime/src/cortex/embedder.rs`, the `/proc/loadavg`-based `chunk_size`/`sleep_ms` branches) is responsible — compare the load average you captured in Step 1/3 against the thresholds in that file (`cores * 0.8` and `cores * 0.4`) to see if the heavy-load branch (`chunk_size=2, sleep_ms=80`) was active, and whether that alone can plausibly account for the observed duration for a ONE-STRING batch (it almost certainly cannot — 80ms is not 100s — so if this branch's time is genuinely dominant, look for something else: e.g., `self.engine.query()` inside `VectorResult` retrieval scanning far more candidates than expected, or a lock on the shared SQLite `cortex_chunks` table contended by another process per Q3).

### Branch D: No single stage dominates (time is spread roughly evenly, or `TOTAL` doesn't match the sum of stages)
→ This points to Q3 (SQLite lock contention or OS-level scheduling contention from the concurrent processes in Fact 4/Step 1), since none of raios's own code should have this shape. If this branch is confirmed:
- Re-run Step 3 with `pgrep -fa raios` immediately before AND after, to see if any of the other concurrent `raios run <agent>` processes were ALSO hitting `~/.config/raios/workspace.db` during your run's window (check their process start times vs. your run's wall-clock window).
- If contention is confirmed, this is a real environmental finding, not a code bug in the search feature itself — report it as such (per systematic-debugging's "When Process Reveals No Root Cause" section) rather than forcing a code fix. The appropriate deliverable in that case is a short, factual report back to Claude Kaira (via `raios handoff`) describing the contention, not a speculative fix.

## Step 5: Fix exactly one thing, re-measure, verify

Whichever branch A-D matched: implement the ONE corresponding fix (not more than one), following `superpowers:test-driven-development` for any code change. Then:

```bash
cargo test --workspace 2>&1 | grep -E "^test result:|FAILED"
cargo check --workspace 2>&1 | tail -5
```

Remove the `[trace]` instrumentation added in Step 2 once the root cause is confirmed and the real fix is in place and verified — it was diagnostic-only, not meant to ship. Re-run the ORIGINAL (uninstrumented) search once more to get a clean final timing number for the fix report.

If the fix doesn't resolve the timing (per the skill: if you're now on fix attempt #2 or later), STOP and return to Step 4 with the new evidence rather than attempting a second speculative fix. If you reach 3 failed fix attempts, stop entirely and report back — do not keep iterating per the skill's explicit "question the architecture" rule.

## Step 6: Report back

Whatever the outcome — root cause found and fixed, or root cause found but out of scope to fix, or genuinely inconclusive after following every step above — write a report and hand off back to Claude Kaira via:

```bash
raios handoff --to claude-kaira --status <success|failed|blocker> -p R-AI-OS --msg "<verbatim findings: which branch A-D matched, the exact trace numbers, what you fixed (if anything), current test count, and the final timing measurement>"
```

Do not merge/push anything without that handoff first — Claude Kaira will review before deciding whether this branch merges to master.
