# Caution-Area Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close four independently-verified gaps in raios: pattern-scanner outputs don't disclose their own limitations, `sigmap` silently ignores its own config and never writes `SIGMAP.md`, `raios usage` can't report Claude Pro/Max quota remaining, and `session_memory.rs` has grown to 974 lines after today's memory port.

**Architecture:** Four independent workstreams, ordered smallest/safest first: (1) CLI output caveat strings, (2) a one-line JSON config fix, (3) a mechanical 3-way file split of `session_memory.rs` along its existing natural seams (transcript I/O / heuristic extraction / L2-L3 distillation), (4) a new local cache bridge — the user's `statusLine` shell script (already reading Claude Code's `rate_limits.*` JSON) writes a small cache file that `raios usage`'s existing `UsageSnapshot`/`UsageSource::LocalLog` machinery (already designed for exactly this) reads back.

**Tech Stack:** Rust (raios-core, raios-runtime, raios-surface-cli crates), POSIX sh (statusLine script), JSON config. No new dependencies anywhere — `std::fs`, `serde_json`, `chrono` are already in every crate touched.

## Global Constraints

- Repo: `/home/alaz/dev/core/R-AI-OS`, currently on branch `master` (today's memory port already merged, HEAD at `35214d4`). Work happens in a **new** isolated git worktree — set one up via `superpowers:using-git-worktrees` before Task 1, same consent-then-`EnterWorktree` flow used earlier this session.
- Task 7 (statusLine cache write) edits `/home/alaz/.claude/settings.json` — a file **outside this git repo**, the user's global cross-session config. Edit it at its real absolute path, not inside the worktree. Be surgical: touch only the `statusLine.command` string, nothing else in the file.
- Task 8 (usage.rs cache read) reads a cache file at `~/.claude/raios-usage-cache.json` — also outside the repo, but only *read*, never written, by Rust code. No git concern there.
- Zero new crates anywhere. `serde_json`, `chrono`, `std::fs`, `dirs` are already dependencies of every crate this plan touches (confirmed: `usage.rs` already imports `chrono::{Local, TimeZone}`, `serde_json::Value`, `std::fs`, and calls `dirs::home_dir()`).
- Verification gate for Tasks 1-3 (CLI output + config): run the actual command and read its output — no Rust test suite involvement.
- Verification gate for Tasks 4-6 (the file split) and Task 8 (usage.rs): `cargo test -p raios-runtime` (and `-p raios-core` where relevant) plus `cargo check --workspace` and `cargo test --workspace` once at the end of each task.
- The file-split tasks (4-6) are **pure moves — no logic changes**. Every function keeps its exact current signature, body, and visibility (`pub`/private) unchanged; only its file location changes. Cut-and-paste from the live file, do not retype function bodies from memory.
- Commit messages: English, conventional-commit style, one commit per task (small tasks may combine steps into one commit — see each task).

---

### Task 1: Security scan confidence footer

**Files:**
- Modify: `crates/raios-surface-cli/src/cli/security/owasp.rs` (end of `cmd_security`, after the `"\nUse --full to see individual issues."` println at ~line 181, before the closing `}` of the function)

**Interfaces:**
- Consumes: nothing new.
- Produces: nothing consumed by later tasks — purely additive CLI output.

- [ ] **Step 1: Read the current end of `cmd_security`**

```bash
sed -n '160,185p' crates/raios-surface-cli/src/cli/security/owasp.rs
```

Confirm the function ends with:
```rust
    if !full && all_reports.iter().any(|(_, _, r)| !r.issues.is_empty()) {
        println!("\nUse --full to see individual issues.");
    }
}
```

- [ ] **Step 2: Add the caveat line**

Change:
```rust
    if !full && all_reports.iter().any(|(_, _, r)| !r.issues.is_empty()) {
        println!("\nUse --full to see individual issues.");
    }
}
```
to:
```rust
    if !full && all_reports.iter().any(|(_, _, r)| !r.issues.is_empty()) {
        println!("\nUse --full to see individual issues.");
    }
    println!("\n⚠ Pattern-based scan — findings need human review; a clean result is not proof of absence.");
}
```

(Placed unconditionally, after the `--full` hint, so it prints whether or not `--full` was used and whether or not any issues were found — the false-confidence risk is highest precisely when the scan reports clean.)

- [ ] **Step 3: Verify manually**

Run: `cargo build -p raios-surface-cli && ./target/debug/raios security 2>&1 | tail -5`

Expected: output ends with the new caveat line. If no projects are registered locally, the command may print "No projects found." on stderr first — that's fine, still confirms the build compiles; if you want to see the full path exercised, pass a real project path: `./target/debug/raios security R-AI-OS 2>&1 | tail -5`.

- [ ] **Step 4: Commit**

```bash
git add crates/raios-surface-cli/src/cli/security/owasp.rs
git commit -m "feat(cli): disclose pattern-based-scan limitation in raios security output"
```

---

### Task 2: Refactor scan confidence footer

**Files:**
- Modify: `crates/raios-surface-cli/src/cli/refactor.rs` (`cmd_refactor`'s non-JSON branch, both the empty-issues early return and the issues-found path)

**Interfaces:**
- Consumes: nothing new.
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Read the current non-JSON branch**

```bash
sed -n '73,98p' crates/raios-surface-cli/src/cli/refactor.rs
```

Confirm it matches:
```rust
    } else {
        if report.issues.is_empty() {
            println!(
                "No refactor issues found. Grade: {} ({}/100)",
                report.grade, report.score
            );
            return;
        }
        println!(
            "Refactor Report — Grade {} ({}/100)",
            report.grade, report.score
        );
        println!(
            "  HIGH: {} | MED: {}",
            report.high_count, report.medium_count
        );
        println!();
        for issue in &report.issues {
            println!(
                "  [{:4}] {} — {}",
                issue.severity.label(),
                issue.file.display(),
                issue.reasons.join("; ")
            );
        }
    }
}
```

- [ ] **Step 2: Add the caveat line to both paths**

Replace the whole block with:
```rust
    } else {
        if report.issues.is_empty() {
            println!(
                "No refactor issues found. Grade: {} ({}/100)",
                report.grade, report.score
            );
            println!("⚠ Pattern-based scan (line-count/nesting/unwrap heuristics) — a clean result is not proof of absence.");
            return;
        }
        println!(
            "Refactor Report — Grade {} ({}/100)",
            report.grade, report.score
        );
        println!(
            "  HIGH: {} | MED: {}",
            report.high_count, report.medium_count
        );
        println!();
        for issue in &report.issues {
            println!(
                "  [{:4}] {} — {}",
                issue.severity.label(),
                issue.file.display(),
                issue.reasons.join("; ")
            );
        }
        println!("\n⚠ Pattern-based scan (line-count/nesting/unwrap heuristics) — findings need human judgment, not all flagged files are actually problems.");
    }
}
```

- [ ] **Step 3: Verify manually**

Run: `cargo build -p raios-surface-cli && ./target/debug/raios refactor . 2>&1 | tail -5`

Expected: output ends with the new caveat line (the "issues found" variant, since this repo currently has HIGH/MED findings per today's `raios refactor R-AI-OS` run).

- [ ] **Step 4: Commit**

```bash
git add crates/raios-surface-cli/src/cli/refactor.rs
git commit -m "feat(cli): disclose pattern-based-scan limitation in raios refactor output"
```

---

### Task 3: Fix sigmap config drift (`customOutput` → `output`)

**Files:**
- Modify: `/home/alaz/dev/core/R-AI-OS/gen-context.config.json` (repo root — inside the worktree, this file IS tracked, confirm with `git ls-files gen-context.config.json`)

**Interfaces:** none — config-only, no code.

**Context:** Verified today: the installed `sigmap` (v6.15.0) does not recognize the `customOutput` key — running `sigmap` prints `[sigmap] unknown config key: "customOutput" (ignored)` and falls back to its built-in default output path, `.github/copilot-instructions.md`. The valid key is `"output"` (a string path). `AGENT_CONSTITUTION.md` (sections 6 and 7, outside this repo) assumes `sigmap` writes `SIGMAP.md` at the project root — restoring that behavior via the correct config key is the smallest fix that keeps the rest of the constitution's SigMap workflow accurate, versus rewriting the global constitution. Also verified: the current run reports `Coverage: D (36%) — 104 of 287 source files included`, because the default `modelContextLimit` (128000) under-budgets a repo this size; bumping it to 200000 (Claude's real context ceiling) should raise the auto-scaled token budget.

- [ ] **Step 1: Confirm current file content**

```bash
cat gen-context.config.json
```

Expected:
```json
{
  "customOutput": "SIGNATURES.md"
}
```

- [ ] **Step 2: Replace with the corrected config**

```json
{
  "output": "SIGMAP.md",
  "modelContextLimit": 200000
}
```

- [ ] **Step 3: Re-run sigmap and verify**

```bash
sigmap 2>&1 | tail -15
```

Expected: no more `unknown config key` warning; output line reads `Output : SIGMAP.md` (not `.github/copilot-instructions.md`); Coverage grade should improve from `D (36%)` (exact new grade depends on repo state at run time — confirm it is no longer D, or if it is still D, confirm the raw file-inclusion count went up from 104/287).

```bash
ls -la SIGMAP.md
git status --short SIGMAP.md gen-context.config.json .github/copilot-instructions.md
```

Expected: `SIGMAP.md` has a fresh mtime; `git status` shows `SIGMAP.md` and `gen-context.config.json` modified. `.github/copilot-instructions.md` should NOT show as modified by this run (sigmap now writes only to the configured `output` target) — if it still changes, note this as a concern in your report rather than silently accepting it, since it would mean `output` didn't fully replace the default target.

- [ ] **Step 4: Note the orphaned `SIGNATURES.md`**

`SIGNATURES.md` (repo root, last modified before this session) is no longer written by any current `sigmap` invocation — it predates the `output` key fix and was never the actual target even under the old (broken) `customOutput` config. Do not delete it — its provenance/purpose isn't fully clear from this investigation alone. Instead, append one line noting this to the Change Log section of `memory.md`.

**Important:** `memory.md` is a tracked file in this repo, and you are working inside a git worktree (a separate checkout on its own branch, sharing history with but physically distinct from the main checkout at `/home/alaz/dev/core/R-AI-OS`). Editing the main checkout's copy of `memory.md` directly would silently bypass this task's commit and leave an untracked, out-of-band change sitting in a different working directory than the one you're operating in. Resolve the correct path first — run `git rev-parse --show-toplevel` from your current working directory and edit `memory.md` at THAT path (your worktree's own tracked copy), never at the hardcoded absolute path `/home/alaz/dev/core/R-AI-OS/memory.md` unless `git rev-parse --show-toplevel` itself resolves to exactly that path:

```markdown
- [2026-07-09] [Claude Kaira]: Fixed sigmap config drift — `customOutput` was an unrecognized key (silently ignored), sigmap actually defaults to writing `.github/copilot-instructions.md`. Corrected to `{"output": "SIGMAP.md", "modelContextLimit": 200000}` to match AGENT_CONSTITUTION's SigMap workflow assumptions and improve coverage grade. `SIGNATURES.md` at repo root is now confirmed orphaned (not written by any current sigmap invocation) — left in place pending a decision on whether to delete it.
```

- [ ] **Step 5: Commit**

```bash
git add gen-context.config.json SIGMAP.md memory.md
git commit -m "fix(sigmap): correct config key so sigmap actually writes SIGMAP.md"
```

---

### Task 4: Extract `transcript_io.rs` from `session_memory.rs`

**Files:**
- Create: `crates/raios-runtime/src/session_memory/mod.rs` (session_memory.rs converts from a single file to a directory module — this matches the existing pattern used by `crates/raios-runtime/src/cortex/{mod.rs,chunker.rs,embedder.rs,store.rs}` and `crates/raios-runtime/src/daemon/{mod.rs,...}` in this same crate)
- Create: `crates/raios-runtime/src/session_memory/transcript_io.rs`
- Modify: `crates/raios-runtime/src/session_memory.rs` (delete after its content is redistributed — becomes `session_memory/mod.rs`)

**Interfaces:**
- Consumes: nothing from other tasks in this plan.
- Produces: `crate::session_memory::transcript_io` submodule containing `claude_project_dir_name`, `find_latest_conversation`, `extract_transcript`, `extract_content_text`, `truncate`, `started_secs`, `read_codex_transcript`, `read_agy_transcript`, `read_opencode_transcript`, `collect_transcript` — all with their EXACT original signatures and visibility, verbatim-moved. `session_memory/mod.rs` re-exports `pub use transcript_io::collect_transcript;` (the only one of these ten functions referenced from outside `raios-runtime` — confirmed via `grep -rn "session_memory::" crates/` today: `crates/raios-runtime/src/session_review.rs:58` calls `crate::session_memory::collect_transcript`). Tasks 5 and 6 build on this same `session_memory/mod.rs` file.

- [ ] **Step 1: Confirm the exact function set and line ranges in the live file**

```bash
grep -n "^pub fn \|^fn \|^pub(crate) fn \|^struct \|^pub struct \|^#\[cfg(test)\]\|^mod tests" crates/raios-runtime/src/session_memory.rs
```

This must show (among others) these ten function signatures somewhere in the file — confirm before moving anything:
`claude_project_dir_name`, `find_latest_conversation`, `extract_transcript`, `extract_content_text`, `truncate`, `started_secs`, `read_codex_transcript`, `read_agy_transcript`, `read_opencode_transcript`, `collect_transcript`.

If the actual function list or signatures differ from what's listed above (the file may have changed since this plan was written), STOP and report NEEDS_CONTEXT with what you actually found — do not improvise a different split.

- [ ] **Step 2: Create the directory module structure**

```bash
mkdir -p crates/raios-runtime/src/session_memory
git mv crates/raios-runtime/src/session_memory.rs crates/raios-runtime/src/session_memory/mod.rs
```

- [ ] **Step 3: Move the ten transcript-I/O functions verbatim**

In `crates/raios-runtime/src/session_memory/mod.rs`, cut (do not retype) the full body of each of the ten functions listed in Step 1, in their current order, and paste them into a new file `crates/raios-runtime/src/session_memory/transcript_io.rs`. Preserve every `use` statement each function needs — check the top of the original `mod.rs` for the imports these ten functions depend on (likely includes `std::path::{Path, PathBuf}`, `std::time::SystemTime`, `serde_json`, `chrono`) and copy the relevant ones to the top of `transcript_io.rs`. Do not guess at unused imports — after the move, `cargo check -p raios-runtime` (Step 5) will flag anything missing or unused on either side.

At the top of `crates/raios-runtime/src/session_memory/mod.rs`, add:
```rust
mod transcript_io;
pub use transcript_io::collect_transcript;
use transcript_io::find_latest_conversation;
```

(`find_latest_conversation` needs a plain `use`, not `pub use` — it's called internally by `post_session_memory_prompt`, which stays in `mod.rs`, per today's grep showing no external caller of `find_latest_conversation` by that path. If `post_session_memory_prompt` or any other function remaining in `mod.rs` calls a different one of the ten moved functions directly, add it to this `use` line too — check by grepping for each function name's call sites within the remaining `mod.rs` content after the cut.)

- [ ] **Step 4: Move the corresponding tests**

If the file's `#[cfg(test)] mod tests { ... }` block (near the end) contains any test functions that exercise `find_latest_conversation`, `extract_transcript`, `collect_transcript`, or the agent-specific transcript readers (`read_codex_transcript`, `read_agy_transcript`, `read_opencode_transcript`), cut those test functions verbatim and move them into a `#[cfg(test)] mod tests { use super::*; ... }` block at the end of the new `transcript_io.rs`. Leave tests for other functions (fact extraction, scene/persona, the periodic-resync regression test) in place in `mod.rs`'s test module for now — Tasks 5 and 6 will relocate those.

- [ ] **Step 5: Verify compilation and tests**

```bash
cargo check -p raios-runtime 2>&1 | tail -40
```

Fix any import errors surfaced (missing `use` on either side of the split — this is expected and normal for a first pass, not a sign something is wrong with the plan).

```bash
cargo test -p raios-runtime 2>&1 | tail -20
```

Expected: same pass count as before this task started (record the pre-task pass count by running `cargo test -p raios-runtime 2>&1 | tail -5` BEFORE Step 2, if you haven't already, so you have a baseline to compare against).

```bash
cargo check --workspace 2>&1 | tail -20
```

Expected: clean, no errors in any crate (confirms `session_review.rs`'s `crate::session_memory::collect_transcript` call and `crates/raios-surface-cli`'s external callers still resolve correctly through the re-export).

- [ ] **Step 6: Commit**

```bash
git add crates/raios-runtime/src/session_memory/
git commit -m "refactor(memory): extract transcript_io module from session_memory"
```

---

### Task 5: Extract `heuristics.rs` from `session_memory/mod.rs`

**Files:**
- Create: `crates/raios-runtime/src/session_memory/heuristics.rs`
- Modify: `crates/raios-runtime/src/session_memory/mod.rs`

**Interfaces:**
- Consumes: the directory-module structure from Task 4.
- Produces: `crate::session_memory::heuristics` submodule containing the `AtomicFact` struct and `first_n_words`, `fnv1a64`, `normalize_fact`, `fact_slug`, `heuristic_extract_facts`, `decision_lines_from_transcript` — exact original signatures/visibility. `mod.rs` re-exports `pub use heuristics::decision_lines_from_transcript;` (the only one of these referenced externally — `crates/raios-runtime/src/session_review.rs:59` calls `crate::session_memory::decision_lines_from_transcript`). `auto_sync_agent_memory` (staying in `mod.rs`, modified by Task 8) needs internal access to `heuristic_extract_facts` and `fact_slug` and `first_n_words` — add a plain `use heuristics::{heuristic_extract_facts, fact_slug, first_n_words, AtomicFact};` in `mod.rs` (adjust the exact list to whatever `mod.rs`'s remaining code actually calls, verified in Step 3 below).

- [ ] **Step 1: Confirm the exact function set in the live file**

```bash
grep -n "^pub fn \|^fn \|^pub(crate) fn \|^struct \|^pub struct " crates/raios-runtime/src/session_memory/mod.rs
```

Confirm `AtomicFact`, `first_n_words`, `fnv1a64`, `normalize_fact`, `fact_slug`, `heuristic_extract_facts`, `decision_lines_from_transcript` are present. If they differ from this list, STOP and report NEEDS_CONTEXT.

- [ ] **Step 2: Move the six items verbatim**

Cut the `AtomicFact` struct and the six function bodies (in their current order) from `mod.rs`, paste into a new `crates/raios-runtime/src/session_memory/heuristics.rs`. Copy whatever `use` statements they depend on to the top of the new file (this file has no external deps beyond `std` — it's pure string/hash logic, confirm no unexpected imports are needed).

- [ ] **Step 3: Wire up `mod.rs`**

Add near the top of `mod.rs` (alongside the `mod transcript_io;` line from Task 4):
```rust
mod heuristics;
pub use heuristics::decision_lines_from_transcript;
use heuristics::{heuristic_extract_facts, fact_slug, first_n_words, AtomicFact};
```

Then grep `mod.rs`'s remaining content for exactly which of `heuristic_extract_facts`, `fact_slug`, `first_n_words`, `AtomicFact` are actually still referenced there (likely inside `auto_sync_agent_memory`'s fact loop) and trim the `use` list to match — an unused `use` produces a compiler warning that Step 5's `cargo check` will surface.

- [ ] **Step 4: Move the corresponding tests**

Move any test functions exercising `heuristic_extract_facts`, `fact_slug`, `normalize_fact`, or `decision_lines_from_transcript` (for example, tests likely named along the lines of `extract_facts_one_per_matched_line`, `fact_slug_is_deterministic_and_normalized`, `decision_lines_still_work` — confirm actual names by reading `mod.rs`'s current test module rather than assuming these exact names) into a `#[cfg(test)] mod tests { use super::*; ... }` block at the end of `heuristics.rs`.

- [ ] **Step 5: Verify**

```bash
cargo check -p raios-runtime 2>&1 | tail -40
cargo test -p raios-runtime 2>&1 | tail -20
cargo check --workspace 2>&1 | tail -20
```

Expected: same total pass count as Task 4's baseline (no tests lost or newly failing), clean workspace check.

- [ ] **Step 6: Commit**

```bash
git add crates/raios-runtime/src/session_memory/
git commit -m "refactor(memory): extract heuristics module from session_memory"
```

---

### Task 6: Extract `distillation.rs` from `session_memory/mod.rs`

**Files:**
- Create: `crates/raios-runtime/src/session_memory/distillation.rs`
- Modify: `crates/raios-runtime/src/session_memory/mod.rs`

**Interfaces:**
- Consumes: the directory-module structure from Tasks 4-5.
- Produces: `crate::session_memory::distillation` submodule containing `upsert_scene_block` and `rebuild_persona` — exact original signatures/visibility. Neither is referenced from outside `raios-runtime` per today's grep, so no `pub use` re-export is needed in `mod.rs` — only a plain `use distillation::{upsert_scene_block, rebuild_persona};` for `auto_sync_agent_memory`'s internal calls (Task 8 will call `rebuild_persona` too, from the same `mod.rs` — this `use` line already covers that).

- [ ] **Step 1: Confirm the exact function set**

```bash
grep -n "^pub fn \|^fn " crates/raios-runtime/src/session_memory/mod.rs
```

Confirm `upsert_scene_block` and `rebuild_persona` are present with their current signatures (`upsert_scene_block(conn: &rusqlite::Connection, project_key: &str, fact_slugs: &[(String, &'static str, String)]) -> Option<String>` and `rebuild_persona(conn: &rusqlite::Connection, project_key: &str) -> Option<()>`, per today's implementation — if signatures differ, STOP and report NEEDS_CONTEXT).

- [ ] **Step 2: Move both functions verbatim**

Cut `upsert_scene_block` and `rebuild_persona` from `mod.rs`, paste into a new `crates/raios-runtime/src/session_memory/distillation.rs`. Copy the `use raios_core::db::{...}` and `use rusqlite` (or equivalent) imports they depend on to the top of the new file.

- [ ] **Step 3: Wire up `mod.rs`**

Add near the top of `mod.rs`:
```rust
mod distillation;
use distillation::{upsert_scene_block, rebuild_persona};
```

- [ ] **Step 4: Move the corresponding tests**

Move any test functions exercising `upsert_scene_block` or `rebuild_persona` (likely `scene_block_upsert_links_facts`, `scene_block_accumulates_across_same_day_calls`, `persona_assembles_from_user_and_feedback_facts` — confirm actual current names by reading the file) into a `#[cfg(test)] mod tests { use super::*; ... }` block at the end of `distillation.rs`. These tests construct an in-memory `rusqlite::Connection` and call `raios_core::db::migrate_existing` — bring those imports along too.

Leave any test that exercises `auto_sync_agent_memory` end-to-end (for example, a periodic-resync regression test that replicates the fact-loop-plus-scene-plus-persona sequence) in `mod.rs`'s own test module — that kind of test belongs with the orchestration logic it's testing, not with any single distilled-out piece.

- [ ] **Step 5: Verify**

```bash
cargo check -p raios-runtime 2>&1 | tail -40
cargo test -p raios-runtime 2>&1 | tail -20
cargo test --workspace 2>&1 | tail -20
cargo check --workspace 2>&1 | tail -20
```

Expected: identical total pass count to the pre-Task-4 baseline across the whole workspace (this is the final verification that the 3-way split changed zero behavior). `mod.rs` should now be small — roughly the original 974 lines minus everything moved into the three new files, containing only `generate_memory_entry`, `append_to_memory_md`, `post_session_memory_prompt`, `cmd_memory_gen`, `auto_sync_agent_memory`, `auto_sync_claude_memory`, the `mod`/`use` wiring, and whatever orchestration-level tests remain.

- [ ] **Step 6: Commit**

```bash
git add crates/raios-runtime/src/session_memory/
git commit -m "refactor(memory): extract distillation module from session_memory, complete the split"
```

---

### Task 7: StatusLine writes a Claude usage cache file

**Files:**
- Modify: `/home/alaz/.claude/settings.json` (absolute path, OUTSIDE this git repo — edit directly, not inside the worktree; this file will not be committed to the R-AI-OS repo)

**Interfaces:**
- Consumes: nothing from this plan's other tasks.
- Produces: a cache file at `~/.claude/raios-usage-cache.json` with shape `{"five_hour_used_pct": <number|null>, "seven_day_used_pct": <number|null>, "five_hour_resets_at": <number|null>, "seven_day_resets_at": <number|null>, "updated_at": "<ISO8601 string>"}`, written best-effort on every statusLine invocation where at least one `rate_limits.*` field is present in the stdin JSON. Task 8 reads this exact file and shape.

**Context:** The current `statusLine.command` (added earlier this session) already parses `.rate_limits.five_hour.used_percentage` and `.rate_limits.seven_day.used_percentage` from stdin JSON via `jq`, to render the visible status line. This task adds a second, independent side-effect to that same script: persist those two values (plus their `resets_at` epoch-seconds fields, not currently read) to a cache file. The cache write must be fire-and-forget — if it fails (disk full, permissions, whatever), the visible status line output must be completely unaffected.

- [ ] **Step 1: Read the current statusLine command**

```bash
python3 -c "
import json
with open('/home/alaz/.claude/settings.json') as f:
    d = json.load(f)
print(d['statusLine']['command'])
"
```

Confirm it matches (modulo any changes since this plan was written — if it looks substantially different, STOP and report NEEDS_CONTEXT rather than guessing):
```
input=$(cat); cwd=$(printf '%s' "$input" | jq -r '.cwd // .workspace.current_dir'); pwd_display=$(printf '%s' "$cwd" | sed "s#^$HOME#~#"); model=$(printf '%s' "$input" | jq -r '.model.display_name // empty'); five=$(printf '%s' "$input" | jq -r '.rate_limits.five_hour.used_percentage // empty'); week=$(printf '%s' "$input" | jq -r '.rate_limits.seven_day.used_percentage // empty'); usage=""; if [ -n "$five" ]; then r5=$(awk -v v="$five" 'BEGIN{printf "%.0f", 100-v}'); usage="5h:${r5}%"; fi; if [ -n "$week" ]; then r7=$(awk -v v="$week" 'BEGIN{printf "%.0f", 100-v}'); if [ -n "$usage" ]; then usage="$usage 7d:${r7}%"; else usage="7d:${r7}%"; fi; fi; out=$(printf '\033[01;34m%s\033[00m' "$pwd_display"); if [ -n "$model" ]; then out="$out  $(printf '\033[01;36m%s\033[00m' "$model")"; fi; if [ -n "$usage" ]; then out="$out  $(printf '\033[00;33m%s\033[00m' "$usage")"; fi; printf '%s' "$out"
```

- [ ] **Step 2: Extend the command with a cache-write side effect**

Replace the `statusLine.command` value with (new content appended before the final `printf '%s' "$out"`, and the two `resets_at` fields newly extracted alongside the existing `five`/`week` extraction):

```
input=$(cat); cwd=$(printf '%s' "$input" | jq -r '.cwd // .workspace.current_dir'); pwd_display=$(printf '%s' "$cwd" | sed "s#^$HOME#~#"); model=$(printf '%s' "$input" | jq -r '.model.display_name // empty'); five=$(printf '%s' "$input" | jq -r '.rate_limits.five_hour.used_percentage // empty'); week=$(printf '%s' "$input" | jq -r '.rate_limits.seven_day.used_percentage // empty'); five_reset=$(printf '%s' "$input" | jq -r '.rate_limits.five_hour.resets_at // empty'); week_reset=$(printf '%s' "$input" | jq -r '.rate_limits.seven_day.resets_at // empty'); usage=""; if [ -n "$five" ]; then r5=$(awk -v v="$five" 'BEGIN{printf "%.0f", 100-v}'); usage="5h:${r5}%"; fi; if [ -n "$week" ]; then r7=$(awk -v v="$week" 'BEGIN{printf "%.0f", 100-v}'); if [ -n "$usage" ]; then usage="$usage 7d:${r7}%"; else usage="7d:${r7}%"; fi; fi; if [ -n "$five" ] || [ -n "$week" ]; then now_iso=$(date -u +%Y-%m-%dT%H:%M:%SZ); five_json=$([ -n "$five" ] && echo "$five" || echo null); week_json=$([ -n "$week" ] && echo "$week" || echo null); five_reset_json=$([ -n "$five_reset" ] && echo "$five_reset" || echo null); week_reset_json=$([ -n "$week_reset" ] && echo "$week_reset" || echo null); printf '{"five_hour_used_pct":%s,"seven_day_used_pct":%s,"five_hour_resets_at":%s,"seven_day_resets_at":%s,"updated_at":"%s"}\n' "$five_json" "$week_json" "$five_reset_json" "$week_reset_json" "$now_iso" > "$HOME/.claude/raios-usage-cache.json" 2>/dev/null || true; fi; out=$(printf '\033[01;34m%s\033[00m' "$pwd_display"); if [ -n "$model" ]; then out="$out  $(printf '\033[01;36m%s\033[00m' "$model")"; fi; if [ -n "$usage" ]; then out="$out  $(printf '\033[00;33m%s\033[00m' "$usage")"; fi; printf '%s' "$out"
```

Key points about this addition: the cache write is wrapped in `if [ -n "$five" ] || [ -n "$week" ]; then ... fi` so it never runs (and never overwrites a previous good cache with nulls) when neither rate-limit field is present in the payload (e.g. session start). The `> "$HOME/.claude/raios-usage-cache.json" 2>/dev/null || true` at the end of the write ensures a permission/disk error is swallowed rather than aborting the script or corrupting `$out`, which is computed and printed entirely afterward regardless of the write's success.

- [ ] **Step 3: Test the extended command directly**

```bash
CMD=$(python3 -c "
import json
with open('/home/alaz/.claude/settings.json') as f:
    d = json.load(f)
print(d['statusLine']['command'])
")
rm -f /tmp/test-usage-cache.json
echo '{"cwd":"/home/alaz/dev/core/R-AI-OS","model":{"display_name":"Sonnet 5"},"rate_limits":{"five_hour":{"used_percentage":18.4,"resets_at":1799999999},"seven_day":{"used_percentage":39,"resets_at":1800500000}}}' | HOME=/tmp bash -c "$CMD"
echo
echo "=== cache file written ==="
cat /tmp/.claude/raios-usage-cache.json 2>/dev/null || echo "MISSING — investigate before proceeding"
```

Expected: the visible statusLine output prints identically to before (cwd + model + usage), AND `/tmp/.claude/raios-usage-cache.json` (created because `HOME` was overridden to `/tmp` for this isolated test) contains valid JSON matching the shape: `{"five_hour_used_pct":18.4,"seven_day_used_pct":39,"five_hour_resets_at":1799999999,"seven_day_resets_at":1800500000,"updated_at":"<some UTC timestamp>"}`.

Also test the no-rate-limits case does NOT write/overwrite the cache:
```bash
mkdir -p /tmp/.claude && echo '{"stale":"marker"}' > /tmp/.claude/raios-usage-cache.json
echo '{"cwd":"/home/alaz/dev/core/R-AI-OS"}' | HOME=/tmp bash -c "$CMD" > /dev/null
cat /tmp/.claude/raios-usage-cache.json
```
Expected: still shows `{"stale":"marker"}` — confirms the script does not touch the cache file when `rate_limits` data is absent from the payload.

Clean up the test artifacts: `rm -rf /tmp/.claude /tmp/test-usage-cache.json`.

- [ ] **Step 4: Apply to the real settings.json**

Use the Edit tool on `/home/alaz/.claude/settings.json` to replace the old `statusLine.command` string with the new one from Step 2 (JSON-escape it correctly — the file is JSON, so the shell command's own double-quotes need to be preserved as `\"` inside the JSON string value, matching how the existing command is already escaped in the file).

Verify the file still parses as valid JSON after the edit:
```bash
python3 -c "import json; json.load(open('/home/alaz/.claude/settings.json')); print('valid JSON')"
```

- [ ] **Step 5: No commit** (this file is outside the git repo). Report the change in your task report instead.

---

### Task 8: `raios usage` reads the Claude quota cache

**Files:**
- Modify: `crates/raios-runtime/src/system_scan/usage.rs` (`scan_claude_usage`, plus a new helper function)
- Test: `crates/raios-runtime/src/system_scan/usage.rs` (inline `#[cfg(test)]` module — check whether one already exists in this file before creating a new one)

**Interfaces:**
- Consumes: the cache file shape from Task 7: `~/.claude/raios-usage-cache.json` = `{"five_hour_used_pct": <number|null>, "seven_day_used_pct": <number|null>, "five_hour_resets_at": <number|null>, "seven_day_resets_at": <number|null>, "updated_at": "<ISO8601>"}`. Existing types this task uses without modification: `UsageSnapshot` (fields: `remaining: Option<String>`, `reset_at: Option<String>`, `confidence: UsageConfidence`, `source: UsageSource`, `notes: Vec<String>` — defined in `crates/raios-runtime/src/system_scan/mod.rs`), `UsageConfidence::{Exact, Estimated, Unavailable}`, `UsageSource::{LocalAuth, Env, LocalLog, Inferred, Unavailable}`.
- Produces: nothing consumed by later tasks — this is the last task in the plan.

**Context:** `scan_claude_usage()` currently calls `apply_claude_auth_metadata(&mut usage, &json)` (if `~/.claude/.credentials.json` exists) which sets `usage.source = UsageSource::LocalAuth`, `usage.plan`, `usage.auth_expires_at`, and pushes a note ending in "...kalan kullanım yerelden okunmuyor" ("remaining usage isn't read locally"). This task adds a second, independent enrichment step after that call: if the Task-7 cache file exists and is fresh enough, populate `usage.remaining` and overwrite `usage.source`/`usage.confidence` to reflect the cache-backed data — WITHOUT clobbering `usage.plan`/`usage.auth_expires_at`, which `apply_claude_auth_metadata` already set correctly from the credentials file.

- [ ] **Step 1: Write the failing test**

Check first whether `usage.rs` already has a `#[cfg(test)] mod tests` block:
```bash
grep -n "mod tests" crates/raios-runtime/src/system_scan/usage.rs
```

If one exists, add the following tests inside it (adjusting `use super::*;` if the existing module already has different imports); if none exists, add this whole block at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_cache(dir: &std::path::Path, content: &str) -> std::path::PathBuf {
        let path = dir.join("raios-usage-cache.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn apply_usage_cache_populates_remaining_from_fresh_cache() {
        let dir = std::env::temp_dir().join(format!("raios-usage-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let cache_path = write_temp_cache(
            &dir,
            &format!(
                r#"{{"five_hour_used_pct":18.4,"seven_day_used_pct":39.0,"five_hour_resets_at":1999999999,"seven_day_resets_at":2000500000,"updated_at":"{}"}}"#,
                now
            ),
        );

        let mut usage = UsageSnapshot::new("Claude Code", true);
        usage.source = UsageSource::LocalAuth;
        usage.plan = Some("pro".into());

        apply_usage_cache(&mut usage, &cache_path);

        assert_eq!(usage.remaining.as_deref(), Some("5h:82% 7d:61% remaining"));
        assert_eq!(usage.source, UsageSource::LocalLog);
        assert_eq!(usage.confidence, UsageConfidence::Estimated);
        // plan set by apply_claude_auth_metadata must survive this enrichment step
        assert_eq!(usage.plan.as_deref(), Some("pro"));
        assert!(usage.notes.iter().any(|n| n.contains("cached from statusLine")));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apply_usage_cache_ignores_stale_cache() {
        let dir = std::env::temp_dir().join(format!("raios-usage-test-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = (chrono::Utc::now() - chrono::Duration::hours(30)).to_rfc3339();
        let cache_path = write_temp_cache(
            &dir,
            &format!(
                r#"{{"five_hour_used_pct":18.4,"seven_day_used_pct":39.0,"five_hour_resets_at":null,"seven_day_resets_at":null,"updated_at":"{}"}}"#,
                old
            ),
        );

        let mut usage = UsageSnapshot::new("Claude Code", true);
        apply_usage_cache(&mut usage, &cache_path);

        assert_eq!(usage.remaining, None);
        assert!(usage.notes.iter().any(|n| n.contains("no active/recent session")));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apply_usage_cache_missing_file_is_a_noop_note() {
        let mut usage = UsageSnapshot::new("Claude Code", true);
        apply_usage_cache(&mut usage, std::path::Path::new("/tmp/does-not-exist-raios-usage-cache.json"));

        assert_eq!(usage.remaining, None);
        assert!(usage.notes.iter().any(|n| n.contains("no active/recent session")));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p raios-runtime apply_usage_cache 2>&1 | tail -20
```

Expected: FAIL to compile — `apply_usage_cache` function not found, and `UsageSource`/`UsageConfidence` may need `PartialEq` derives to support the `assert_eq!` calls (check `crates/raios-runtime/src/system_scan/mod.rs` — if `UsageConfidence`/`UsageSource` don't already derive `PartialEq`, add it to both enum definitions; they currently derive `Debug, Clone, Serialize` per today's investigation, so this is an additive derive, safe to add).

- [ ] **Step 3: Implement `apply_usage_cache`**

Add this function to `crates/raios-runtime/src/system_scan/usage.rs`, near `apply_claude_auth_metadata`:

```rust
const USAGE_CACHE_STALENESS_HOURS: i64 = 24;

fn apply_usage_cache(usage: &mut UsageSnapshot, cache_path: &Path) {
    let Ok(content) = fs::read_to_string(cache_path) else {
        usage.notes.push(
            "Kalan kullanım cache dosyası bulunamadı — no active/recent session to source from."
                .into(),
        );
        return;
    };
    let Ok(cache) = serde_json::from_str::<Value>(&content) else {
        usage.notes.push(
            "Kalan kullanım cache dosyası okunamadı (bozuk JSON) — no active/recent session to source from."
                .into(),
        );
        return;
    };

    let updated_at = cache.get("updated_at").and_then(Value::as_str);
    let is_fresh = updated_at
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| {
            let age = chrono::Utc::now().signed_duration_since(dt.with_timezone(&chrono::Utc));
            age.num_hours() < USAGE_CACHE_STALENESS_HOURS
        })
        .unwrap_or(false);

    if !is_fresh {
        usage.notes.push(
            "Kalan kullanım cache'i eski (>24 saat) — no active/recent session to source from."
                .into(),
        );
        return;
    }

    let five = cache.get("five_hour_used_pct").and_then(Value::as_f64);
    let week = cache.get("seven_day_used_pct").and_then(Value::as_f64);

    if five.is_none() && week.is_none() {
        usage.notes.push(
            "Kalan kullanım cache'inde veri yok — no active/recent session to source from.".into(),
        );
        return;
    }

    let mut parts = Vec::new();
    if let Some(v) = five {
        parts.push(format!("5h:{:.0}%", 100.0 - v));
    }
    if let Some(v) = week {
        parts.push(format!("7d:{:.0}%", 100.0 - v));
    }
    usage.remaining = Some(format!("{} remaining", parts.join(" ")));
    usage.source = UsageSource::LocalLog;
    usage.confidence = UsageConfidence::Estimated;
    usage.notes.push(format!(
        "cached from statusLine as of {}, not live-polled — only reflects the last active Claude Code session tick.",
        updated_at.unwrap_or("unknown time")
    ));
}
```

If `UsageConfidence` and `UsageSource` in `crates/raios-runtime/src/system_scan/mod.rs` don't already derive `PartialEq`, update both enum declarations there (this is the only change to `mod.rs` in this task):
```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum UsageConfidence {
```
and
```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum UsageSource {
```

- [ ] **Step 4: Wire it into `scan_claude_usage`**

In `scan_claude_usage()`, after the existing `if let Some(json) = read_json_value(&creds_path) { apply_claude_auth_metadata(&mut usage, &json); }` block, add:

```rust
    let cache_path = home.join(".claude/raios-usage-cache.json");
    apply_usage_cache(&mut usage, &cache_path);

    usage
```

(replacing the bare `usage` that currently ends the function — this must be the LAST step before returning, so it runs after `apply_claude_auth_metadata` and can see/preserve the `plan`/`auth_expires_at` fields that call already set).

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p raios-runtime apply_usage_cache 2>&1 | tail -20
```

Expected: all 3 new tests PASS.

- [ ] **Step 6: Run the full verification gate**

```bash
cargo test -p raios-runtime 2>&1 | tail -10
cargo check --workspace 2>&1 | tail -10
cargo test --workspace 2>&1 | tail -10
```

Expected: all green, no regressions.

- [ ] **Step 7: Manual end-to-end sanity check**

If a real `~/.claude/raios-usage-cache.json` exists on this machine from Task 7's statusLine already having run at least once in a live session since Task 7 was applied:
```bash
cargo build -p raios-surface-cli && ./target/debug/raios usage 2>&1 | grep -A 3 "Claude Code"
```
Expected: the `REMAINING` column for Claude Code now shows something like `5h:XX% 7d:YY% remaining` instead of `unknown` (if the cache is fresh), or still `unknown` with a note explaining why (if no cache exists yet, or it's stale) — either outcome is correct depending on real cache state; the important thing is it no longer says "kalan kullanım yerelden okunmuyor" unconditionally when a fresh cache IS available.

- [ ] **Step 8: Commit**

```bash
git add crates/raios-runtime/src/system_scan/usage.rs crates/raios-runtime/src/system_scan/mod.rs
git commit -m "feat(usage): read cached Claude rate-limit data from statusLine for raios usage"
```

---

## Final Verification (all tasks)

- [ ] Run `cargo test --workspace` once more from a clean state and confirm the total test count is (baseline from before Task 4) + 3 (Task 8's new tests), with 0 failures.
- [ ] Run `raios security . 2>&1 | tail -3`, `raios refactor . 2>&1 | tail -3`, `sigmap 2>&1 | tail -5`, and `raios usage 2>&1 | head -6` once each and eyeball that all four outputs look as described in their respective tasks.
- [ ] Proceed to `superpowers:finishing-a-development-branch` for merge/PR/cleanup, same as the earlier plan today.
