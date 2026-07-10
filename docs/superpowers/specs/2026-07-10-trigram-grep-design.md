# Trigram Grep Index — Design

**Date:** 2026-07-10
**Status:** Approved for planning
**Assignee:** Codex Kaira
**Sibling project (parallel, independent):** Cortex daemon residency (`2026-07-10-cortex-daemon-design.md`, Antigravity Kaira). Coordination note at the bottom.

## Problem

raios has no exact-match/substring/regex search. BM25 is word-token-based: `getUserById` is stored as one token, so a query for the substring `UserById` finds nothing. Agents therefore fall back to `grep`/`rg` for every exact-match need. Goal: a `raios grep` that answers arbitrary substring and regex queries in milliseconds over an indexed project — the missing "fast + exhaustive + exact" layer, complementary to BM25 (word relevance) and Cortex (semantic).

## How trigram search works (the whole trick)

Index time: every file's content is broken into all consecutive 3-character windows ("trigrams"), lowercased; the index stores `trigram → set of files containing it`. Query time: extract the literal substrings the pattern *requires* (e.g. `error.*timeout` requires `error` and `timeout`), trigram those literals, intersect the file sets from the index (a few ms of SQL), then run the real regex only on the few candidate files to produce exact line matches. Files are never scanned unless they're candidates.

## Goals

- `raios grep <pattern> [--dir <path>] [-i] [--reindex]` — exact/regex search, project-scoped by default (cwd), explicit dir with `--dir`, matching `raios search`'s scope semantics.
- Exhaustive within scope: returns EVERY matching line (path, line number, line text) — grep semantics, not top-k relevance.
- Millisecond-class query latency on a warm index for patterns with extractable literals.
- Graceful fallback: patterns with no extractable ≥3-char literal (e.g. `a.b`, pure alternations) fall back to a full scoped walk + regex scan — always correct, just slower.
- Same cache discipline as the (just-shipped) BM25 cache: mtime-incremental, scope-aware reads (component-aware `Path::starts_with`, canonicalized root), single write transaction, `force` param, shared `workspace.db`.
- MCP tool `grep_search` exposing the same capability to agents.

## Non-Goals

- Replacing BM25 or Cortex — this is a third, parallel engine for a different question type.
- Multiline patterns, PCRE features beyond Rust `regex` crate support, binary-file search.
- Touching `cmd_search`/`search.rs` — this project adds a NEW subcommand with its own files, specifically to avoid merge conflicts with the parallel Cortex-daemon project which modifies `cmd_search`.

## Design

### Storage (new tables in shared `workspace.db`, created idempotently by the new module itself, mirroring `open_bm25_db`'s pattern)

```sql
CREATE TABLE IF NOT EXISTS trigram_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT UNIQUE NOT NULL,          -- canonical absolute path
    mtime_secs INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS trigram_postings (
    trigram TEXT NOT NULL,              -- 3 chars, lowercased
    file_id INTEGER NOT NULL REFERENCES trigram_files(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_trigram ON trigram_postings(trigram);
CREATE UNIQUE INDEX IF NOT EXISTS idx_trigram_file ON trigram_postings(trigram, file_id);
```

Per-file DISTINCT trigram set only (no line numbers in the index — line-level results come from scanning candidate files at query time). This keeps the index dramatically smaller than per-occurrence storage; dedup via the unique index + `INSERT OR IGNORE`.

Case handling: trigrams stored lowercased. A lowercased-trigram index yields a superset of candidates for both case-sensitive and case-insensitive queries; the verify stage applies exact case semantics. Case-sensitive correctness is never lost — only candidate sets are slightly wider.

### Module: `crates/raios-runtime/src/search/trigram.rs` (new file)

```rust
pub struct GrepMatch { pub path: PathBuf, pub line_no: usize, pub line: String }

pub fn grep(root: &Path, db_path: &Path, pattern: &str,
            case_insensitive: bool, force: bool) -> anyhow::Result<Vec<GrepMatch>>
```

Pipeline inside `grep`:
1. `ensure_index(root, db_path, force)` — canonicalize root once; scope-filtered cache read (`retain` + `Path::starts_with`); mtime diff; re-trigram changed/new files; delete rows for vanished in-scope files; all writes in one `unchecked_transaction`. Walk rules identical to BM25's (`max_depth(6)`, same `SKIP_DIRS`, same `INDEXED_EXTS` — reuse the constants from `indexer.rs` by making them `pub(crate)` rather than copying).
2. `extract_required_literals(pattern) -> Option<Vec<String>>` — hand-rolled scanner (NO new crates; `regex-syntax` is not a direct dependency and must not be added): walk the pattern; `\` escapes of ordinary chars contribute a literal char while class escapes (`\d`, `\w`, `\s`, `\b` …) terminate the current run; the metacharacters `. * + ? ( ) [ ] { } | ^ $` terminate the current run; a `?`/`*`/`{0`-quantifier makes the *preceding* run's last char optional, so drop that char from the run; any top-level `|` → return `None` (no literal is guaranteed across alternation). Collect completed runs; keep those with ≥3 chars. `None` or empty → fallback.
3. Candidate selection: trigram every kept literal (lowercased), then
   `SELECT file_id FROM trigram_postings WHERE trigram IN (…) GROUP BY file_id HAVING COUNT(DISTINCT trigram) = ?n` — files containing ALL required trigrams (ANDing across multiple literals is sound: every required literal must appear somewhere in a matching file).
4. Verify: compile the pattern once with `regex::RegexBuilder::new(pattern).case_insensitive(case_insensitive)` (the `regex` crate is already a raios-runtime dependency, `Cargo.toml:19`); scan each candidate file line-by-line; emit `GrepMatch` per matching line. Fallback path (step 2 returned `None`): same verify scan over ALL in-scope files from the walker instead of index candidates.

### CLI: `raios grep` (new subcommand — new file `crates/raios-surface-cli/src/cli/grep.rs`)

Args: `pattern: String`, `--dir Option<PathBuf>` (default cwd, same resolution as `raios search`), `-i/--ignore-case`, `--reindex`, plus the global `--json`. Human output mirrors grep: `path:line_no:line`. JSON output: array of `{path, line, text}`.

### MCP: `grep_search` tool (in `tools_workspace.rs` + tool list in `tools.rs`)

Schema: `{ pattern (required), path (optional, same resolve_search_scope helper as semantic_search), case_insensitive (optional bool) }`. Calls the same `trigram::grep`.

## Testing

Unit (in `trigram.rs`): literal extraction table — `foobar`→`["foobar"]`; `error.*timeout`→`["error","timeout"]`; `f.b`→None-or-<3-char-runs→fallback; `foo|bar`→`None`; `foo\.bar`→`["foo.bar"]`; `colou?r`→`["colo"]` (optional char dropped); escape handling `\d+items`→`["items"]`. Index tests mirroring the proven BM25 set: cache reuse, mtime reindex, scope isolation, sibling-prefix (`R-AI-OS` vs `R-AI-OS-fork`) safety, force rebuild no-duplication — all on TempDir DBs, never the real `workspace.db`. End-to-end: known small workspace, assert exact match sets for a literal, a regex with literals, a case-insensitive query, and a fallback-only pattern; assert a non-matching candidate (trigram hit but regex miss) is correctly excluded. Release-mode timing demo in final verification: warm literal query over R-AI-OS must complete well under 1s.

## Risks & Coordination

- Literal-extraction scanner is deliberately conservative — anything confusing → fallback scan (correct, slower). Never trade correctness for candidate narrowing.
- Index size: distinct-trigrams-per-file bounded (~thousands/file); watch total `workspace.db` growth in final verification and report the delta.
- **Parallel-project conflict surface:** the Cortex-daemon project (Antigravity) modifies `cli/search.rs`, `daemon/`, `cortex/`. This project must NOT touch those files. Shared-touch files are only `cli/args.rs`, `cli/mod.rs` (one new subcommand arm each) and `mcp/tools.rs`/`tools_workspace.rs` (one new tool each) — keep those diffs minimal and append-style. Whoever merges second rebases onto master and resolves the small overlap.
