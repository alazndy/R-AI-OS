# Trigram Grep Index Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Design doc (read first for the "why"): `docs/superpowers/specs/2026-07-10-trigram-grep-design.md`.

**Goal:** `raios grep <pattern>` — millisecond, exhaustive, exact/regex search over an mtime-cached trigram index in the shared `workspace.db`, with a correct full-scan fallback for patterns that yield no usable literals.

**Architecture:** One new engine module (`search/trigram.rs`: schema, indexer, literal extractor, candidate intersection, regex verify), one new CLI subcommand (`cli/grep.rs`), one new MCP tool (`grep_search`). Nothing existing is modified except constant visibility in `indexer.rs` and append-only registrations in `args.rs`/`mod.rs`/`tools*.rs`.

**Tech Stack:** Rust; `rusqlite`, `walkdir`, `anyhow`, `regex` (ALL already dependencies — `regex = "1.10"` at `crates/raios-runtime/Cargo.toml:19`). **No new crates — in particular do NOT add `regex-syntax`;** the literal extractor is hand-rolled below.

## Global Constraints

- Repo `/home/alaz/dev/core/R-AI-OS`, branch from current `master`. Work in a NEW isolated git worktree.
- **Parallel project running simultaneously** (Antigravity: Cortex daemon residency). DO NOT touch: `cli/search.rs`, `cortex/`, `daemon/`, `bin/aiosd.rs`. Your additions to shared files (`cli/args.rs`, `cli/mod.rs`, `mcp/tools.rs`, `mcp/tools_workspace.rs`) must be append-style and minimal. Whoever merges second rebases and resolves.
- Every test uses TempDir-isolated db paths — never the real `~/.config/raios/workspace.db`.
- Scope comparisons: component-aware `Path::starts_with` on a once-canonicalized root. Never string prefixes, never SQL LIKE.
- All index writes in one `unchecked_transaction` (codebase precedent: `db/mem.rs:53`, `cortex/store.rs:224`, and the BM25 write phase in `search/indexer.rs` shipped today — read that function first, this plan's cache discipline deliberately mirrors it).
- `cargo test --workspace` + `cargo check --workspace` clean after every task. Baseline today: 619 tests. Commit per task, conventional-commit English messages.

---

### Task 1: Literal extractor (pure function + table tests)

**Files:**
- Create: `crates/raios-runtime/src/search/trigram.rs`
- Modify: `crates/raios-runtime/src/search/mod.rs` (add `pub mod trigram;`)

**Interfaces:**
- Produces: `pub(crate) fn extract_required_literals(pattern: &str) -> Option<Vec<String>>` — `Some(runs)` where every returned string (each ≥3 chars) MUST appear in any match of `pattern`; `None` when no such guarantee exists (top-level alternation, or no run reaches 3 chars). Task 4 consumes this.

- [ ] **Step 1: Write the failing table test** (bottom of the new file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_extraction_table() {
        let cases: Vec<(&str, Option<Vec<&str>>)> = vec![
            ("foobar", Some(vec!["foobar"])),
            ("error.*timeout", Some(vec!["error", "timeout"])),
            ("f.b", Some(vec![])),              // runs "f","b" both <3 chars → empty (routes to fallback)
            ("foo|bar", None),                  // alternation: nothing guaranteed
            ("(foo|bar)baz", None),             // alternation ANYWHERE → None (conservative: "baz" is
                                                // technically guaranteed but we forgo the optimization
                                                // rather than risk over-claiming inside groups)
            (r"foo\.bar", Some(vec!["foo.bar"])), // escaped meta char is a literal char
            ("colou?r", Some(vec!["colo"])),    // '?' makes preceding char optional: drop 'u', 'r' alone <3
            (r"\d+items", Some(vec!["items"])), // class escape breaks the run
            ("getUser.*ById", Some(vec!["getUser", "ById"])),
            ("ab(cd)ef", Some(vec![])),         // groups break runs; "ab","cd","ef" all <3 → Some but empty
            ("", None),
        ];
        for (pat, want) in cases {
            let got = extract_required_literals(pat);
            let want = want.map(|v| v.into_iter().map(String::from).collect::<Vec<_>>());
            assert_eq!(got, want, "pattern: {pat:?}");
        }
    }
}
```

Decision codified by the table: `Some(vec![])` (runs existed but all <3 chars) and `None` both route to fallback in Task 4 — but `None` specifically means "alternation/empty, nothing is guaranteed," kept distinct for clarity.

- [ ] **Step 2:** `cargo test -p raios-runtime trigram` → FAIL (function missing).

- [ ] **Step 3: Implement** (top of `trigram.rs`):

```rust
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

/// Extract literal runs that MUST appear in any match of `pattern`.
/// Conservative: anything ambiguous → shorter runs or None. Never over-claims.
pub(crate) fn extract_required_literals(pattern: &str) -> Option<Vec<String>> {
    if pattern.is_empty() {
        return None;
    }
    let mut runs: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = pattern.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                // class escapes / anchors end the run; escaped ordinary or meta chars are literal
                Some('d' | 'D' | 'w' | 'W' | 's' | 'S' | 'b' | 'B' | 'A' | 'z') | None => {
                    if !cur.is_empty() { runs.push(std::mem::take(&mut cur)); }
                }
                Some(esc) => cur.push(esc),
            },
            // Alternation ANYWHERE → bail. Inside a group, neither branch is guaranteed,
            // and text after the group IS guaranteed — but tracking that correctly needs
            // real parsing. Conservative: any '|' means no literal guarantees at all.
            '|' => return None,
            '(' | ')' => { if !cur.is_empty() { runs.push(std::mem::take(&mut cur)); } }
            '?' | '*' => {
                // preceding char is optional (or repeated-from-zero): it is NOT required
                cur.pop();
                if !cur.is_empty() { runs.push(std::mem::take(&mut cur)); }
            }
            '{' => {
                // {0,..} or {0} makes preceding char optional; any other {n.. keeps it.
                // Conservative: treat like '?' (drop preceding char), then skip to '}'.
                cur.pop();
                if !cur.is_empty() { runs.push(std::mem::take(&mut cur)); }
                for c2 in chars.by_ref() { if c2 == '}' { break; } }
            }
            '.' | '+' | '[' | ']' | '^' | '$' => {
                if c == '[' {
                    for c2 in chars.by_ref() { if c2 == ']' { break; } }
                }
                if c == '+' {
                    // 'x+' requires at least one 'x': the char stays required — do NOT pop.
                    continue;
                }
                if !cur.is_empty() { runs.push(std::mem::take(&mut cur)); }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { runs.push(cur); }
    Some(runs.into_iter().filter(|r| r.chars().count() >= 3).collect())
}
```

Note on `+`: `x+` means "one or more x" — the char IS required, so `+` doesn't end or trim the run; it just contributes nothing further. Re-check the table: `\d+items` — `\d` ends the run (empty), `+` continues, `items` accumulates → `["items"]` ✓.

- [ ] **Step 4:** `cargo test -p raios-runtime trigram` → PASS. Also `pub mod trigram;` added to `search/mod.rs`, `cargo check --workspace` clean.

- [ ] **Step 5:** `git add -A crates/raios-runtime/src/search/ && git commit -m "feat(grep): conservative required-literal extractor for trigram candidate selection"`

---

### Task 2: Trigram index — schema, cold build, mtime cache, scope, force

**Files:**
- Modify: `crates/raios-runtime/src/search/trigram.rs`
- Modify: `crates/raios-runtime/src/search/indexer.rs` (visibility only: `const INDEXED_EXTS`/`const SKIP_DIRS` → `pub(crate) const`, and make `fs_mtimes` `pub(crate)` — reuse, don't copy)

**Interfaces:**
- Consumes: `indexer::{INDEXED_EXTS, SKIP_DIRS, fs_mtimes}` (after visibility change).
- Produces: `pub(crate) fn ensure_index(root: &Path, db_path: &Path, force: bool) -> Result<PathBuf>` — returns the canonicalized root; guarantees the index reflects current in-scope file mtimes. `fn trigrams_of(content: &str) -> impl Iterator/HashSet<String>` (distinct, lowercased 3-char windows). Task 4 consumes both.

- [ ] **Step 1: Failing tests** — mirror today's proven BM25 suite exactly (read `indexer.rs`'s test module first, copy its TempDir/make-workspace idioms): `cold_build_then_cache_reuse` (second call touches no rows: compare `trigram_files` rowids), `modified_file_reindexes`, `scope_isolation_survives_other_project`, `sibling_prefix_not_matched` (`R-AI-OS` vs `R-AI-OS-fork`), `force_rebuild_no_duplication`, plus `trigrams_are_distinct_and_lowercased` (`"AbcAbc"` → exactly `{"abc","bca","cab"}`).

- [ ] **Step 2:** run → FAIL (functions missing).

- [ ] **Step 3: Implement.** Schema/open (mirror `open_bm25_db`):

```rust
fn open_trigram_db(db_path: &Path) -> Result<Connection> {
    if let Some(p) = db_path.parent() { let _ = std::fs::create_dir_all(p); }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS trigram_files (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             path TEXT UNIQUE NOT NULL,
             mtime_secs INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS trigram_postings (
             trigram TEXT NOT NULL,
             file_id INTEGER NOT NULL REFERENCES trigram_files(id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS idx_trigram ON trigram_postings(trigram);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_trigram_file ON trigram_postings(trigram, file_id);",
    )?;
    Ok(conn)
}

fn trigrams_of(content: &str) -> std::collections::HashSet<String> {
    let lower: Vec<char> = content.to_lowercase().chars().collect();
    lower.windows(3).map(|w| w.iter().collect()).collect()
}
```

`ensure_index(root, db_path, force)`: canonicalize root once (`std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())`); `fs_mtimes(&root)` (reused from indexer); read cached rows `SELECT id, path, mtime_secs FROM trigram_files`, `retain` to `Path::new(path).starts_with(&root)`; when `!force` classify warm (mtime equal) vs stale (missing/changed → collect ids); when `force` treat all in-scope as needing re-index (delete nothing, `INSERT OR REPLACE` refreshes — same reasoning as today's BM25 force). One `unchecked_transaction`: DELETE stale ids; for each non-warm fs file → read content, `trigrams_of`, `INSERT OR REPLACE INTO trigram_files`, `last_insert_rowid`, then `INSERT OR IGNORE INTO trigram_postings` per distinct trigram; `tx.commit()`. Return the canonical root.

- [ ] **Step 4:** tests PASS; full `cargo test -p raios-runtime` no regressions; `cargo check --workspace` clean (visibility changes compile everywhere).

- [ ] **Step 5:** `git commit -m "feat(grep): mtime-cached, scope-safe trigram index in shared workspace.db"`

---

### Task 3: Candidate selection + verify + `grep()` end-to-end

**Files:**
- Modify: `crates/raios-runtime/src/search/trigram.rs`

**Interfaces:**
- Produces (Tasks 4-5 call this exact signature):
```rust
pub struct GrepMatch { pub path: PathBuf, pub line_no: usize, pub line: String }
pub fn grep(root: &Path, db_path: &Path, pattern: &str,
            case_insensitive: bool, force: bool) -> Result<Vec<GrepMatch>>
```

- [ ] **Step 1: Failing tests:** a TempDir workspace with 3 files of known content; assert exact `(path, line_no)` sets for: literal query (`"getUserById"` present in 2 files), regex with literals (`"fn get.*Id"`), case-insensitive hit that case-sensitive misses, fallback-only pattern (`"a.c"` with a planted `"abc"` — must still be found via full scan), and a trigram-candidate-but-regex-miss file (contains `error` and `timeout` on DIFFERENT lines while pattern is `error.*timeout` — single-line matching must exclude it). Also: results must be sorted (path, then line_no) for determinism.

- [ ] **Step 2:** FAIL (types missing).

- [ ] **Step 3: Implement:**

```rust
pub fn grep(root: &Path, db_path: &Path, pattern: &str,
            case_insensitive: bool, force: bool) -> Result<Vec<GrepMatch>> {
    let root = ensure_index(root, db_path, force)?;
    let re = regex::RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| anyhow::anyhow!("invalid pattern: {e}"))?;

    let candidates: Vec<PathBuf> = match extract_required_literals(pattern) {
        Some(literals) if !literals.is_empty() => {
            let mut tris: std::collections::HashSet<String> = std::collections::HashSet::new();
            for lit in &literals { tris.extend(trigrams_of(lit)); }
            let conn = open_trigram_db(db_path)?;
            let placeholders = vec!["?"; tris.len()].join(",");
            let sql = format!(
                "SELECT f.path FROM trigram_postings p
                 JOIN trigram_files f ON f.id = p.file_id
                 WHERE p.trigram IN ({placeholders})
                 GROUP BY p.file_id HAVING COUNT(DISTINCT p.trigram) = ?"
            );
            let mut stmt = conn.prepare(&sql)?;
            let params_vec: Vec<&dyn rusqlite::ToSql> = tris.iter()
                .map(|t| t as &dyn rusqlite::ToSql).collect();
            let n = tris.len() as i64;
            let mut all: Vec<&dyn rusqlite::ToSql> = params_vec; all.push(&n);
            stmt.query_map(all.as_slice(), |r| r.get::<_, String>(0))?
                .flatten()
                .map(PathBuf::from)
                .filter(|p| p.starts_with(&root)) // scope re-check: index is shared across projects
                .collect()
        }
        // Some(vec![]) or None → fallback: every in-scope file
        _ => crate::search::indexer::fs_mtimes(&root).keys().map(PathBuf::from).collect(),
    };

    let mut out = Vec::new();
    for path in candidates {
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                out.push(GrepMatch { path: path.clone(), line_no: i + 1, line: line.to_string() });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path).then(a.line_no.cmp(&b.line_no)));
    Ok(out)
}
```

The `filter(|p| p.starts_with(&root))` after the SQL is load-bearing: the trigram tables are shared across every project ever indexed, and the SQL has no scope column — candidates from other projects must be dropped in Rust, same pattern as `Cortex::search_scoped`.

- [ ] **Step 4:** tests PASS; workspace suites clean.
- [ ] **Step 5:** `git commit -m "feat(grep): trigram candidate intersection + regex verify with full-scan fallback"`

---

### Task 4: CLI subcommand `raios grep`

**Files:**
- Create: `crates/raios-surface-cli/src/cli/grep.rs`
- Modify (append-only): `crates/raios-surface-cli/src/cli/args.rs` (new `Grep` variant after `Search` — model the field shapes on `Search`'s existing `query/top_k/reindex/dir`), `crates/raios-surface-cli/src/cli/mod.rs` (one `mod grep;` + one match arm)

- [ ] **Step 1:** Read `args.rs`'s `Search` variant and `mod.rs`'s `Commands::Search` arm as shipped today (both changed this week — read fresh, don't assume). Add:

```rust
    /// Exact/regex search over the trigram index (grep-fast, exhaustive within scope)
    Grep {
        pattern: String,
        /// Directory to scan (defaults to the current working directory)
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
        #[arg(short = 'i', long)]
        ignore_case: bool,
        #[arg(long)]
        reindex: bool,
    },
```

Dispatch arm (in `mod.rs`, mirroring `Search`'s scope default): `let scope = dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| cfg.dev_ops_path.clone()));` → `grep::cmd_grep(&pattern, &scope, ignore_case, reindex, cli.json)`.

- [ ] **Step 2:** `cmd_grep` in the new `cli/grep.rs`: call `raios_runtime::search::trigram::grep(scope, &raios_runtime::cortex::store::default_db_path(), pattern, ignore_case, reindex)` (NOTE: `trigram::grep` must therefore be `pub`, and `search/mod.rs` may need a `pub use` — adjust visibility from Task 1's `pub(crate)` where the compiler demands, keeping `extract_required_literals` crate-private). Human output: `println!("{}:{}:{}", m.path.display(), m.line_no, m.line)`; `--json`: array of `{path, line, text}`; empty → `eprintln!("no matches")`, exit code stays 0 (match grep? grep exits 1 on no-match — do NOT replicate; raios subcommands don't use exit codes semantically, keep 0).

- [ ] **Step 3:** `cargo build -p raios-surface-cli`; manual: `./target/debug/raios grep "load_or_build" --dir .` → expect known hits in `indexer.rs` + call sites; `./target/debug/raios grep "LOAD_OR_BUILD" -i --dir .` same hits; `./target/debug/raios grep "zzz_nothing_zzz" --dir .` → no matches.
- [ ] **Step 4:** `git commit -m "feat(cli): raios grep subcommand over the trigram index"`

---

### Task 5: MCP tool `grep_search`

**Files:**
- Modify (append-only): `crates/raios-surface-mcp/src/mcp/tools.rs` (tool JSON entry + dispatch arm — read how `semantic_search` registers at tools.rs:64 and dispatches at tools.rs:148), `crates/raios-surface-mcp/src/mcp/tools_workspace.rs` (handler)

- [ ] **Step 1:** Tool entry: name `grep_search`, description "Exact/regex code search (grep-equivalent, trigram-indexed, exhaustive within scope). Defaults to the current project — pass path for another project/directory.", schema `{pattern (required string), path (optional string), case_insensitive (optional bool)}`.
- [ ] **Step 2:** Handler `tool_grep_search`: reuse the existing `resolve_search_scope(&args)` helper (shipped today in the same file); call `trigram::grep(&scope, &default_db_path(), pattern, ci, false)`; cap the response (first 200 matches + a `"truncated": true` note if more — MCP replies must not explode on a 10k-match query); return the standard `{"content":[{"type":"text","text": …}]}` shape with a summary line + pretty JSON.
- [ ] **Step 3:** `cargo check --workspace` clean.
- [ ] **Step 4:** `git commit -m "feat(mcp): grep_search tool for agents"`

---

### Task 6: Final verification

- [ ] `cargo test --workspace` — baseline 619 + this plan's new tests, 0 fail; record exact count.
- [ ] `cargo build --release -p raios-surface-cli`; timing demo on the real repo: `time ./target/release/raios grep "unchecked_transaction" --dir .` twice — first run pays indexing, second (warm) must be well under 1s; record both numbers. Cross-check correctness against `grep -rn "unchecked_transaction" --include="*.rs" crates/` — the raios result must contain every .rs hit grep finds under the same scope (allowing for the walker's depth/skip rules — investigate and explain any diff rather than waving it off).
- [ ] Report `workspace.db` size before/after first index of this repo (`ls -la ~/.config/... ` — NO, tests never touch the real DB; this measurement uses the release binary's real run above, which legitimately does).
- [ ] Hand off back to claude-kaira via `raios handoff` with: final test count, both timings, the grep-vs-grep correctness result, and DB size delta. Do not merge/push without that handoff.
