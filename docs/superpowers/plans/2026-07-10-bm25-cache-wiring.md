# BM25 Cache Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the existing, tested, currently-unused `ProjectIndex::load_or_build()` into `raios search` and `semantic_search`'s real call sites, fixing four latent defects (cross-scope cache eviction, path-form cache-key mismatch, a quadratic/unbatched cold write, and a missing `--reindex`/force path) that make it unsafe to wire as-is today.

**Architecture:** Five surgical changes to `crates/raios-runtime/src/search/indexer.rs` (scope-aware cache read, path canonicalization, a returning `index_file`, a single write transaction, a `force` parameter), then two one-line call-site swaps from `ProjectIndex::build(scope)` to `ProjectIndex::load_or_build(scope, &db_path, force)`.

**Tech Stack:** Rust, `rusqlite` (already a dependency, transaction API only), no new crates.

## Global Constraints

- Repo: `/home/alaz/dev/core/R-AI-OS`. Design doc: `docs/superpowers/specs/2026-07-10-bm25-cache-wiring-design.md` (read it first for the "why" — this plan is the "how").
- No new crates. `rusqlite`, `walkdir`, `anyhow` already used in `indexer.rs`.
- Single shared SQLite file at `~/.config/raios/workspace.db` (via `default_db_path()`, made `pub` in Task 1) is the real target — every test in this plan MUST use an isolated `TempDir`-based db path, never that real file.
- Work in a **new** isolated git worktree — `superpowers:using-git-worktrees` consent flow, or `git worktree add` if no native tool is available in your harness. Branch from current `master`.
- `Path::starts_with` (component-aware) for every scope comparison — never a raw string prefix, never SQL `LIKE`. This codebase already hit and fixed the identical bug class in `Cortex::search_scoped` (`crates/raios-runtime/src/cortex/mod.rs`) — a naive prefix check lets `"R-AI-OS-fork"` match scope `"R-AI-OS"`.
- `cargo test --workspace` and `cargo check --workspace` must stay clean after every task.
- Commit after every task. English, conventional-commit messages.

---

### Task 1: Expose `default_db_path()`

**Files:**
- Modify: `crates/raios-runtime/src/cortex/store.rs:9-14`

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub fn raios_runtime::cortex::store::default_db_path() -> PathBuf` — Tasks 7 and 8 call this exact path to get the shared DB location.

- [ ] **Step 1: Confirm current code**

```bash
sed -n '1,20p' crates/raios-runtime/src/cortex/store.rs
```

Confirm it matches:
```rust
fn default_db_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("raios")
        .join("workspace.db")
}
```

- [ ] **Step 2: Make it public**

Change:
```rust
fn default_db_path() -> PathBuf {
```
to:
```rust
pub fn default_db_path() -> PathBuf {
```

- [ ] **Step 3: Verify it's reachable from the crate root**

```bash
grep -n "pub mod store" crates/raios-runtime/src/cortex/mod.rs
grep -n "pub mod cortex" crates/raios-runtime/src/lib.rs
```

Both must show `pub`. If either doesn't, STOP and report — the plan's import path (`raios_runtime::cortex::store::default_db_path()`, used in Tasks 7-8) depends on both being public; do not change their visibility yourself without checking what else might rely on them staying private first.

- [ ] **Step 4: Verify compilation**

```bash
cargo check -p raios-runtime 2>&1 | tail -20
```

Expected: clean, no errors, no new warnings (an unused-`pub`-fn warning would NOT appear yet since nothing calls it until Task 7 — that's expected and fine).

- [ ] **Step 5: Commit**

```bash
git add crates/raios-runtime/src/cortex/store.rs
git commit -m "feat(search): expose default_db_path for BM25 cache wiring"
```

---

### Task 2: Scope-aware cache read + path canonicalization

**Files:**
- Modify: `crates/raios-runtime/src/search/indexer.rs:161-186` (top of `load_or_build`)
- Test: `crates/raios-runtime/src/search/indexer.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: `load_or_build`'s internal `cached` map is now scope-filtered before the stale-detection loop runs, and `root` is canonicalized once at function entry — Tasks 3-5 build on this same function body, working further down inside it.

**Context:** Today, `load_cached_bm25_files(&conn)` (indexer.rs:334-353) returns every row in `bm25_files` regardless of scope. The stale-detection loop right after it (`for (path, (file_id, cached_mtime, _)) in &cached { match fs.get(path.as_str()) { ... } }`) treats any cached row NOT found in the current call's `fs_mtimes(root)` map as stale and deletes it — so calling `load_or_build` for project B deletes project A's cached rows from the shared DB. Separately, `fs_mtimes(root)` stores keys exactly as `WalkDir` yields them, which depends on whether `root` was passed as relative (`.`) or absolute — the same file gets different cache keys on different invocations.

- [ ] **Step 1: Write the failing tests**

Read the current test module first:

```bash
sed -n '355,412p' crates/raios-runtime/src/search/indexer.rs
```

Confirm it has `make_workspace(tmp: &TempDir) -> PathBuf` and the three existing tests. Add these two new tests at the end of the `mod tests` block (before the closing `}`):

```rust
    #[test]
    fn scope_isolation_survives_a_different_project_being_indexed() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("test.db");

        let ws_a = tmp.path().join("a");
        fs::create_dir_all(&ws_a).unwrap();
        fs::write(ws_a.join("main.rs"), "fn main() { println!(\"from a\"); }").unwrap();

        let ws_b = tmp.path().join("b");
        fs::create_dir_all(&ws_b).unwrap();
        fs::write(ws_b.join("main.rs"), "fn main() { println!(\"from b\"); }").unwrap();

        let idx_a1 = ProjectIndex::load_or_build(&ws_a, &db, false).unwrap();
        assert_eq!(idx_a1.doc_count, 1);

        // Indexing B must not evict A's cached rows.
        ProjectIndex::load_or_build(&ws_b, &db, false).unwrap();

        let conn = Connection::open(&db).unwrap();
        let a_path = ws_a.join("main.rs").canonicalize().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bm25_files WHERE path = ?1",
                params![a_path.to_string_lossy().to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "project A's cached row must survive indexing project B");

        // Re-loading A must warm-reuse the surviving cache, not rebuild from scratch.
        let idx_a2 = ProjectIndex::load_or_build(&ws_a, &db, false).unwrap();
        assert_eq!(idx_a2.doc_count, idx_a1.doc_count);
        let results = idx_a2.search("println");
        assert!(!results.is_empty());
    }

    #[test]
    fn scope_filter_does_not_match_sibling_dir_with_shared_prefix() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("test.db");

        let ws = tmp.path().join("R-AI-OS");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("main.rs"), "fn main() { println!(\"core\"); }").unwrap();

        let ws_fork = tmp.path().join("R-AI-OS-fork");
        fs::create_dir_all(&ws_fork).unwrap();
        fs::write(ws_fork.join("main.rs"), "fn main() { println!(\"fork\"); }").unwrap();

        ProjectIndex::load_or_build(&ws, &db, false).unwrap();
        ProjectIndex::load_or_build(&ws_fork, &db, false).unwrap();

        let conn = Connection::open(&db).unwrap();
        let core_path = ws.join("main.rs").canonicalize().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bm25_files WHERE path = ?1",
                params![core_path.to_string_lossy().to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "R-AI-OS's cached row must survive indexing the sibling R-AI-OS-fork directory"
        );
    }
```

Add `use rusqlite::Connection;` to the test module's imports if not already present (check `use super::*;` at the top of `mod tests` — `Connection` and `params` come from the outer `use rusqlite::{params, Connection};` at indexer.rs:2, which `use super::*;` already re-imports, so no new import line should be needed — verify this compiles before adding anything redundant).

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p raios-runtime scope_isolation_survives scope_filter_does_not_match_sibling 2>&1 | tail -30
```

Expected: FAIL — `scope_isolation_survives_a_different_project_being_indexed` asserts `count, 1` but gets `count, 0` (A's row was deleted when B was indexed); `scope_filter_does_not_match_sibling_dir_with_shared_prefix` fails the same way.

- [ ] **Step 3: Implement the fix**

Read the current function start:

```bash
sed -n '161,165p' crates/raios-runtime/src/search/indexer.rs
```

Confirm it matches:
```rust
    pub fn load_or_build(root: &Path, db_path: &Path) -> Result<Self> {
        let conn = open_bm25_db(db_path)?;
        let fs = fs_mtimes(root);
        let cached = load_cached_bm25_files(&conn);
```

Replace with:
```rust
    pub fn load_or_build(root: &Path, db_path: &Path) -> Result<Self> {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let conn = open_bm25_db(db_path)?;
        let fs = fs_mtimes(&root);
        let mut cached = load_cached_bm25_files(&conn);
        cached.retain(|path, _| Path::new(path).starts_with(&root));
```

(This only touches these 4 lines. `root` is now an owned, canonical `PathBuf` shadowing the `&Path` parameter — every other place in the function that reads `root` by value or reference keeps working unchanged since `PathBuf` derefs to `&Path`. `load_or_build`'s signature itself is unchanged in this task — Task 5 adds the `force` parameter later.)

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p raios-runtime scope_isolation_survives scope_filter_does_not_match_sibling 2>&1 | tail -20
```

Expected: both PASS.

- [ ] **Step 5: Run the full existing indexer test suite**

```bash
cargo test -p raios-runtime search::indexer 2>&1 | tail -20
```

Expected: all pass, including the 3 pre-existing tests (`load_or_build_creates_index`, `second_load_uses_cache`, `modified_file_triggers_reindex`) — they're unaffected since this task didn't change the function signature, only its internal scope-filtering.

- [ ] **Step 6: Commit**

```bash
git add crates/raios-runtime/src/search/indexer.rs
git commit -m "fix(search): scope BM25 cache reads to prevent cross-project eviction"
```

---

### Task 3: `index_file` returns its postings

**Files:**
- Modify: `crates/raios-runtime/src/search/indexer.rs:78-99`

**Interfaces:**
- Consumes: nothing new.
- Produces: `fn index_file(&mut self, path: PathBuf, content: &str) -> Vec<(String, usize, String)>` (token, line_no, snippet — one entry per token occurrence in the file). Task 4 uses this return value directly.

**Context:** `index_file` currently only mutates `self.inverted`; nothing captures which entries it just added. `load_or_build`'s cold-write loop (Task 4) needs to know exactly which postings belong to the file it just indexed, without re-scanning the whole `self.inverted` map.

- [ ] **Step 1: Confirm current code**

```bash
sed -n '78,99p' crates/raios-runtime/src/search/indexer.rs
```

Confirm it matches:
```rust
    fn index_file(&mut self, path: PathBuf, content: &str) {
        let file_id = self.files.len();
        self.files.push(path);
        self.doc_count += 1;

        let mut total_tokens = 0usize;

        for (line_no, line) in content.lines().enumerate() {
            let tokens = tokenize(line);
            total_tokens += tokens.len();
            let snippet: String = line.trim().chars().take(100).collect();
            for token in tokens {
                self.inverted.entry(token).or_default().push((
                    file_id,
                    line_no + 1,
                    snippet.clone(),
                ));
            }
        }

        self.doc_lengths.push(total_tokens.max(1));
    }
```

- [ ] **Step 2: Change the signature and collect the return value**

Replace with:
```rust
    fn index_file(&mut self, path: PathBuf, content: &str) -> Vec<(String, usize, String)> {
        let file_id = self.files.len();
        self.files.push(path);
        self.doc_count += 1;

        let mut total_tokens = 0usize;
        let mut new_postings: Vec<(String, usize, String)> = Vec::new();

        for (line_no, line) in content.lines().enumerate() {
            let tokens = tokenize(line);
            total_tokens += tokens.len();
            let snippet: String = line.trim().chars().take(100).collect();
            for token in tokens {
                self.inverted.entry(token.clone()).or_default().push((
                    file_id,
                    line_no + 1,
                    snippet.clone(),
                ));
                new_postings.push((token, line_no + 1, snippet.clone()));
            }
        }

        self.doc_lengths.push(total_tokens.max(1));
        new_postings
    }
```

(Note `token.clone()` on the `self.inverted.entry(...)` call — `token` is moved into `new_postings.push` afterward, so the entry call needs its own copy. This keeps `self.inverted` populated exactly as before, for `search()` and the two other read paths that depend on it, while also returning the same data.)

- [ ] **Step 3: Fix the `build()` call site**

```bash
grep -n "index_file" crates/raios-runtime/src/search/indexer.rs
```

Find `build()`'s call (around line 45-77). It currently reads `idx.index_file(path.to_path_buf(), &content);` as a bare statement — `Vec<T>` is not `#[must_use]`, so this compiles unchanged with no warning. Confirm this directly rather than assuming:

```bash
cargo check -p raios-runtime 2>&1 | grep -i "warning.*unused\|must_use"
```

Expected: no output (no new warnings). If any warning appears referencing this call site, add `let _ =` in front of it in `build()` and re-run this check.

- [ ] **Step 4: Verify compilation and existing tests**

```bash
cargo check -p raios-runtime 2>&1 | tail -20
cargo test -p raios-runtime search::indexer 2>&1 | tail -20
```

Expected: clean build, all existing indexer tests (including Task 2's two new ones) still pass — `index_file`'s behavior toward `self.inverted`/`self.doc_lengths`/`self.files` is unchanged, only its return type changed.

- [ ] **Step 5: Commit**

```bash
git add crates/raios-runtime/src/search/indexer.rs
git commit -m "refactor(search): index_file returns its postings for O(1) cold-write lookup"
```

---

### Task 4: Transactional, non-quadratic write phase

**Files:**
- Modify: `crates/raios-runtime/src/search/indexer.rs:188-190` (stale deletes) and `:228-253` (new-file insert loop, using Task 2's line numbers as a starting reference — re-read the file fresh since Tasks 2-3 shifted line numbers)
- Test: `crates/raios-runtime/src/search/indexer.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `index_file`'s new return type from Task 3 (`Vec<(String, usize, String)>`).
- Produces: nothing new for later tasks — this task's deliverable is purely internal correctness/performance, verified by its own test.

**Context:** Today, for each newly-indexed file, the code scans the ENTIRE `idx.inverted` map (every token across every warm AND new file) filtering by `posting_slot == slot` to find just this one file's postings — O(new_files × total_postings). Each stale-delete and each insert is also its own autocommit statement. This task uses Task 3's returned `Vec` directly (O(1) per file, no scan) and wraps every write in one transaction, following this codebase's established `unchecked_transaction()` pattern (see `crates/raios-core/src/db/mem.rs:53` or `crates/raios-runtime/src/cortex/store.rs:224` for the exact idiom already used elsewhere).

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `crates/raios-runtime/src/search/indexer.rs`:

```rust
    #[test]
    fn cold_build_writes_exactly_the_right_number_of_postings() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("test.db");
        let ws = tmp.path().join("ws");
        fs::create_dir_all(&ws).unwrap();
        // "fn main" -> 2 tokens (>=3 chars each: "fn" is 2 chars, filtered out by tokenize;
        // confirm tokenize's >=3-char rule by using words that clear it unambiguously)
        fs::write(&ws.join("a.rs"), "alpha bravo").unwrap();
        fs::write(&ws.join("b.rs"), "charlie delta echo").unwrap();

        ProjectIndex::load_or_build(&ws, &db, false).unwrap();

        let conn = Connection::open(&db).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bm25_postings", [], |r| r.get(0))
            .unwrap();
        // a.rs: alpha, bravo = 2 tokens. b.rs: charlie, delta, echo = 3 tokens. Total 5.
        assert_eq!(count, 5, "posting count must equal exact per-file token occurrences, no duplication or loss");
    }
```

- [ ] **Step 2: Run the test to verify it fails or passes for the wrong reason**

```bash
cargo test -p raios-runtime cold_build_writes_exactly_the_right_number_of_postings 2>&1 | tail -20
```

This may actually PASS against today's unoptimized code (the quadratic scan is slow but not incorrect) — that's fine and expected; this test's purpose is to guard the upcoming refactor against introducing a correctness bug (writing wrong/duplicate/missing postings), not to prove the current code is broken. Proceed regardless of pass/fail here — the goal is a regression guard for Step 3.

- [ ] **Step 3: Implement the transactional rewrite**

Read the current write phase fresh (line numbers have shifted from Tasks 2-3):

```bash
grep -n "stale_ids\|for (path_str, &fs_mtime) in &fs\|fn load_or_build" crates/raios-runtime/src/search/indexer.rs
```

Locate the block from the `stale_ids` deletion loop through the end of `load_or_build` (ending at `Ok(idx)`). It currently looks like this (adjust to whatever the actual current content is if it has drifted — the shape must match this before you touch it):

```rust
        for id in &stale_ids {
            let _ = conn.execute("DELETE FROM bm25_files WHERE id = ?1", params![id]);
        }

        // ... warm-path loading (unchanged, do not touch) ...

        for (path_str, &fs_mtime) in &fs {
            if warm_paths.contains(path_str) {
                continue;
            }
            let path = PathBuf::from(path_str);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let slot = idx.files.len();
                idx.index_file(path.clone(), &content);
                let doc_len = idx.doc_lengths.get(slot).copied().unwrap_or(1);

                let _ = conn.execute(
                    "INSERT OR REPLACE INTO bm25_files (path, mtime_secs, doc_length) VALUES (?1,?2,?3)",
                    params![path_str, fs_mtime as i64, doc_len as i64],
                );
                let file_id = conn.last_insert_rowid();
                for (token, postings) in &idx.inverted {
                    for &(posting_slot, line_no, ref snippet) in postings {
                        if posting_slot == slot {
                            let _ = conn.execute(
                                "INSERT INTO bm25_postings (token, file_id, line_no, snippet) VALUES (?1,?2,?3,?4)",
                                params![token, file_id, line_no as i64, snippet],
                            );
                        }
                    }
                }
            }
        }

        Ok(idx)
    }
```

Replace the `stale_ids` deletion loop AND the new-file insert loop (leave the warm-path loading block between them untouched) with:

```rust
        let tx = conn.unchecked_transaction()?;
        for id in &stale_ids {
            let _ = tx.execute("DELETE FROM bm25_files WHERE id = ?1", params![id]);
        }
```

(this replaces just the `for id in &stale_ids { ... }` block — keep everything between it and the new-file loop exactly as-is)

then replace the new-file insert loop with:

```rust
        for (path_str, &fs_mtime) in &fs {
            if warm_paths.contains(path_str) {
                continue;
            }
            let path = PathBuf::from(path_str);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let postings = idx.index_file(path.clone(), &content);
                let doc_len = idx.doc_lengths.last().copied().unwrap_or(1);

                let _ = tx.execute(
                    "INSERT OR REPLACE INTO bm25_files (path, mtime_secs, doc_length) VALUES (?1,?2,?3)",
                    params![path_str, fs_mtime as i64, doc_len as i64],
                );
                let file_id = tx.last_insert_rowid();
                for (token, line_no, snippet) in &postings {
                    let _ = tx.execute(
                        "INSERT INTO bm25_postings (token, file_id, line_no, snippet) VALUES (?1,?2,?3,?4)",
                        params![token, file_id, *line_no as i64, snippet],
                    );
                }
            }
        }
        tx.commit()?;

        Ok(idx)
    }
```

(`idx.doc_lengths.last()` replaces the old `idx.doc_lengths.get(slot)` — since `index_file` just pushed exactly one new entry onto `doc_lengths`, `.last()` is that entry; `slot` is no longer computed/needed anywhere in this loop since `postings` from Task 3's return value replaces the old scan-by-slot approach entirely.)

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p raios-runtime cold_build_writes_exactly_the_right_number_of_postings 2>&1 | tail -20
```

Expected: PASS, count == 5.

- [ ] **Step 5: Run the full indexer test suite**

```bash
cargo test -p raios-runtime search::indexer 2>&1 | tail -30
```

Expected: all pass — the 3 original tests, Task 2's 2 scope tests, and this task's new test.

- [ ] **Step 6: Commit**

```bash
git add crates/raios-runtime/src/search/indexer.rs
git commit -m "perf(search): wrap BM25 cache writes in one transaction, drop the O(n²) posting scan"
```

---

### Task 5: `force` parameter, wire `--reindex`

**Files:**
- Modify: `crates/raios-runtime/src/search/indexer.rs` (`load_or_build`'s signature and the warm-path computation)
- Test: `crates/raios-runtime/src/search/indexer.rs` (update 3 existing tests' call sites, add 1 new test)

**Interfaces:**
- Consumes: nothing new from Tasks 1-4 beyond the already-modified `load_or_build`.
- Produces: `pub fn load_or_build(root: &Path, db_path: &Path, force: bool) -> Result<Self>` — Tasks 7 and 8 call this exact 3-argument signature.

**Context:** `--reindex` today only forces Cortex's re-embedding; BM25 has no equivalent. `force: true` must skip warm-path detection entirely (every file gets freshly re-tokenized) while still respecting Task 2's scope filter — force means "rebuild fresh within this scope," not "touch the whole shared DB."

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
    #[test]
    fn force_rebuild_replaces_rather_than_duplicates() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");

        ProjectIndex::load_or_build(&ws, &db, true).unwrap();
        let idx2 = ProjectIndex::load_or_build(&ws, &db, true).unwrap();

        let conn = Connection::open(&db).unwrap();
        let file_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bm25_files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(file_count, 2, "REPLACE semantics must not duplicate rows across repeated force rebuilds");

        let results = idx2.search("println");
        assert!(!results.is_empty());
    }
```

Also update the 3 EXISTING tests' calls from `ProjectIndex::load_or_build(&ws, &db)` to `ProjectIndex::load_or_build(&ws, &db, false)` — find them:

```bash
grep -n "load_or_build(&ws, &db)\|load_or_build(&ws_a, &db)\|load_or_build(&ws_b, &db)\|load_or_build(&ws_fork, &db)" crates/raios-runtime/src/search/indexer.rs
```

The 3 original tests (`load_or_build_creates_index`, `second_load_uses_cache`, `modified_file_triggers_reindex`) each call `ProjectIndex::load_or_build(&ws, &db)` — change every one of those specific 2-argument calls to add `, false` as the third argument. (Task 2's two tests and Task 4's test already call the function with the old 2-argument form too if you wrote them before this task — update every `load_or_build(...)` call site across the whole test module to take 3 arguments, `false` unless the test is specifically about force.)

- [ ] **Step 2: Run tests to verify compilation fails**

```bash
cargo test -p raios-runtime search::indexer 2>&1 | tail -30
```

Expected: FAIL to compile — `load_or_build` still takes 2 arguments, the new test and updated calls pass 3.

- [ ] **Step 3: Add the `force` parameter**

```bash
sed -n '161,165p' crates/raios-runtime/src/search/indexer.rs
```

Confirm current state (post-Task-2):
```rust
    pub fn load_or_build(root: &Path, db_path: &Path) -> Result<Self> {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let conn = open_bm25_db(db_path)?;
        let fs = fs_mtimes(&root);
        let mut cached = load_cached_bm25_files(&conn);
        cached.retain(|path, _| Path::new(path).starts_with(&root));
```

Change the signature line only:
```rust
    pub fn load_or_build(root: &Path, db_path: &Path, force: bool) -> Result<Self> {
```

Then find the warm/stale computation loop (right after the lines above):

```bash
grep -n "for (path, (file_id, cached_mtime, _)) in &cached" crates/raios-runtime/src/search/indexer.rs
```

It looks like:
```rust
        for (path, (file_id, cached_mtime, _)) in &cached {
            match fs.get(path.as_str()) {
                Some(&fs_mtime) if fs_mtime == *cached_mtime => {
                    warm_paths.insert(path.clone());
                }
                _ => {
                    stale_ids.push(*file_id);
                }
            }
        }
```

Wrap it with a `force` check — when `force` is true, every scope-filtered cached row is treated as needing fresh indexing (not warm), but is NOT pushed to `stale_ids` either (force is not the same as "this file vanished," so don't issue a DELETE for it — `INSERT OR REPLACE` in the write phase already overwrites it cleanly once re-indexed):

```rust
        if !force {
            for (path, (file_id, cached_mtime, _)) in &cached {
                match fs.get(path.as_str()) {
                    Some(&fs_mtime) if fs_mtime == *cached_mtime => {
                        warm_paths.insert(path.clone());
                    }
                    _ => {
                        stale_ids.push(*file_id);
                    }
                }
            }
        }
```

(`warm_paths` and `stale_ids` are declared just above this loop as empty `HashSet`/`Vec` — when `force` is true, this whole block is skipped, so both stay empty: `warm_paths.contains(path_str)` in the write-phase loop is always false, so every file in `fs` gets freshly re-indexed via `INSERT OR REPLACE`, and `stale_ids` stays empty so nothing gets explicitly deleted. This is scope-safe because `cached` was already narrowed to this scope by Task 2's `retain` — force never touches rows outside `root`.)

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p raios-runtime search::indexer 2>&1 | tail -30
```

Expected: all pass, including the new `force_rebuild_replaces_rather_than_duplicates` test.

- [ ] **Step 5: Commit**

```bash
git add crates/raios-runtime/src/search/indexer.rs
git commit -m "feat(search): add force parameter to load_or_build for --reindex support"
```

---

### Task 6: Wire the CLI call site

**Files:**
- Modify: `crates/raios-surface-cli/src/cli/search.rs`

**Interfaces:**
- Consumes: `raios_runtime::cortex::store::default_db_path() -> PathBuf` (Task 1), `raios_runtime::indexer::ProjectIndex::load_or_build(root: &Path, db_path: &Path, force: bool) -> Result<ProjectIndex>` (Tasks 2-5).
- Produces: nothing new — this is a leaf call-site change.

- [ ] **Step 1: Confirm current code**

```bash
sed -n '1,40p' crates/raios-surface-cli/src/cli/search.rs
```

Confirm `cmd_search`'s signature is `pub(super) fn cmd_search(query: &str, top_k: usize, reindex: bool, scope: &Path, json: bool)` and it currently has:
```rust
    let bm25_hits = match raios_runtime::indexer::ProjectIndex::build(scope) {
        Ok(idx) => idx.search(query),
        Err(e) => {
            eprintln!("Index build failed: {e}");
            vec![]
        }
    };
```

If either differs from this, STOP and report NEEDS_CONTEXT with what you actually found.

- [ ] **Step 2: Swap the call**

Change:
```rust
    let bm25_hits = match raios_runtime::indexer::ProjectIndex::build(scope) {
```
to:
```rust
    let bm25_hits = match raios_runtime::indexer::ProjectIndex::load_or_build(
        scope,
        &raios_runtime::cortex::store::default_db_path(),
        reindex,
    ) {
```

(leave the `Ok(idx) => idx.search(query), Err(e) => { ... }` arms unchanged — only the function call itself changes.)

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p raios-surface-cli 2>&1 | tail -30
```

Expected: clean.

- [ ] **Step 4: Manual verification**

```bash
cargo build --release -p raios-surface-cli 2>&1 | tail -5
time ./target/release/raios search "memory layer" --top-k 5 2>&1 | tail -10
echo "=== second run, should be noticeably faster on the BM25 side (warm cache) ==="
time ./target/release/raios search "memory layer" --top-k 5 2>&1 | tail -10
```

Expected: both runs succeed, correctly scoped results (no cross-project leakage — verify by eye that every result path is under the current directory), second run's wall-clock time is equal or lower than the first (Cortex was already warm-cached before this change; this task additionally makes the BM25 side warm-cached on the second call).

- [ ] **Step 5: Commit**

```bash
git add crates/raios-surface-cli/src/cli/search.rs
git commit -m "feat(cli): wire raios search to the cached, scope-safe BM25 index"
```

---

### Task 7: Wire the MCP call site

**Files:**
- Modify: `crates/raios-surface-mcp/src/mcp/tools_workspace.rs`

**Interfaces:**
- Consumes: same as Task 6 — `raios_runtime::cortex::store::default_db_path()`, `raios_runtime::search::indexer::ProjectIndex::load_or_build(root, db_path, force)`.
- Produces: nothing new — leaf call-site change.

- [ ] **Step 1: Confirm current code**

```bash
grep -n "ProjectIndex::build" crates/raios-surface-mcp/src/mcp/tools_workspace.rs
```

Confirm it's:
```rust
        let bm25_hits = raios_runtime::search::indexer::ProjectIndex::build(&scope)
            .map_err(|e| format!("BM25 index build failed: {e}"))?
            .search(query);
```

If it differs, STOP and report NEEDS_CONTEXT.

- [ ] **Step 2: Swap the call**

Change to:
```rust
        let bm25_hits = raios_runtime::search::indexer::ProjectIndex::load_or_build(
            &scope,
            &raios_runtime::cortex::store::default_db_path(),
            false,
        )
        .map_err(|e| format!("BM25 index build failed: {e}"))?
        .search(query);
```

(`false` for `force` — per the design's Non-Goals, the MCP surface has no reindex flag; not adding one now, YAGNI.)

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p raios-surface-mcp 2>&1 | tail -30
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/raios-surface-mcp/src/mcp/tools_workspace.rs
git commit -m "feat(mcp): wire semantic_search to the cached, scope-safe BM25 index"
```

---

### Task 8: Final verification

**Files:** none — verification only.

- [ ] **Step 1: Full workspace test suite**

```bash
cargo test --workspace 2>&1 | grep -E "^test result:|FAILED|error\[|panicked"
```

Expected: every crate's `test result: ok`, 0 failed. Compare the total pass count against the baseline you recorded when starting this plan (today's session baseline before this plan: 615 tests workspace-wide) — expect it to have grown by exactly the number of new tests added across Tasks 2, 4, and 5 (2 + 1 + 1 = 4 new tests, so ~619 total; confirm the exact number yourself rather than trusting this arithmetic blindly).

- [ ] **Step 2: Full workspace compile check**

```bash
cargo check --workspace 2>&1 | tail -20
```

Expected: clean, no warnings introduced by this plan's changes.

- [ ] **Step 3: Release-mode timing verification (not just unit tests)**

```bash
cargo build --release -p raios-surface-cli 2>&1 | tail -5
cd /home/alaz/dev/core/R-AI-OS   # or wherever this worktree's checkout lives — search a real project directory
echo "=== cold (or already-warm-from-Task-6) run ==="
time ./target/release/raios search "memory layer" --top-k 5 2>&1 | tail -3
echo "=== second run, both Cortex and BM25 now warm ==="
time ./target/release/raios search "memory layer" --top-k 5 2>&1 | tail -3
```

Record both timings in your final report. Expected: both complete in low single-digit seconds (today's session baseline: ~3.9s release, dominated by Cortex's unconditional HNSW rebuild, which this plan does NOT change — the BM25 portion's contribution to total time should now be near-zero on the second call specifically, since it's cache-warm; the plan does not aim to change the Cortex-dominated total, only to stop BM25 from adding its own uncached cost on top).

- [ ] **Step 4: Report**

Write a final summary covering: exact final test count, both timing numbers from Step 3, and confirmation that all 8 tasks' individual test suites still pass together (not just individually — a full `cargo test --workspace` run IS that confirmation, already done in Step 1).
