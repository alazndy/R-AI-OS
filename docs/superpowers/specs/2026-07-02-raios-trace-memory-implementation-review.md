# R-AI-OS Trace Memory Implementation Review

Reviewer: Claude Kaira
Author: Codex Kaira
Date: 2026-07-02
Repo: `/home/alaz/dev/core/R-AI-OS`

## Review Goal

Review the implemented Trace Memory feature for correctness, schema compatibility, security posture, and memory-pollution risk.

This is an implementation review, not a planning review. Please focus on bugs, behavioral regressions, unsafe storage, migration issues, and missing tests.

## Implemented Scope

1. Central trace persistence:
   - New `tool_traces` table in `crates/raios-core/src/db/schema.rs`.
   - New DB module `crates/raios-core/src/db/tool_traces.rs`.
   - CRUD/search functions:
     - `tool_trace_insert`
     - `tool_trace_record_secret_refusal`
     - `tool_trace_get`
     - `tool_trace_forget`
     - `tool_trace_search`
   - Exact dedupe via SHA-256 `content_hash`.
   - Secret-like trace input is refused and stored only as a redacted refusal row.

2. CLI:
   - New `raios trace` subcommands in `crates/raios-surface-cli/src/cli/trace.rs`.
   - Actions wired through:
     - `crates/raios-surface-cli/src/cli/action_types.rs`
     - `crates/raios-surface-cli/src/cli/args.rs`
     - `crates/raios-surface-cli/src/cli/mod.rs`
   - Commands:
     - `raios trace record`
     - `raios trace search`
     - `raios trace forget`
     - `raios trace kg-export`

3. Runtime recall:
   - New `crates/raios-runtime/src/trace_recall.rs`.
   - `relevant_trace_block()` searches successful non-redacted traces.
   - Search fallback order:
     - full query within project
     - significant query tokens within project
     - project key
     - full query across projects
   - Handoff creation appends relevant trace memory to the stored handoff message.
   - Handoff delivery in `raios run` augments `[HANDOVER CONTEXT]`.

4. Session-review auto trace:
   - `raios run` records a trace from post-run review only when:
     - the run failed, or
     - review has risks, or
     - review has learned decisions.
   - Empty successful sessions are intentionally skipped to avoid memory pollution.

5. Trace to Evolution bridge:
   - `raios evolve from-traces [--project] [-n]`.
   - Converts successful non-redacted trace fixes into pending instinct candidates.
   - Does not auto-promote candidates.
   - Found and fixed schema split:
     - core migration creates `instinct_candidates(project_name, command, outcome, suggestion, status, created_at)`
     - runtime `CandidateStore` previously expected `rule/source/confidence/expires_at/promoted`
     - `CandidateStore` now supports both schemas.

6. MemPalace bridge:
   - `raios trace kg-export [query] [--project] [-n]`.
   - Emits JSON triples with:
     - `subject`
     - `predicate`
     - `object`
     - `valid_from`
     - `source.kind = "raios_tool_trace"`
     - `source.trace_id`
   - Export-only by design: R-AI-OS Rust CLI is not an MCP client and should not silently write to MemPalace.

## Files To Review

Core:
- `crates/raios-core/src/db/schema.rs`
- `crates/raios-core/src/db/mod.rs`
- `crates/raios-core/src/db/tool_traces.rs`
- `crates/raios-core/src/db/tests/tool_traces.rs`
- `crates/raios-core/src/db/tests/schema.rs`
- `crates/raios-core/src/db/tests/mod.rs`

Runtime:
- `crates/raios-runtime/src/trace_recall.rs`
- `crates/raios-runtime/src/agent_runner.rs`
- `crates/raios-runtime/src/intelligence/evolution.rs`
- `crates/raios-runtime/src/lib.rs`

CLI:
- `crates/raios-surface-cli/src/cli/trace.rs`
- `crates/raios-surface-cli/src/cli/handoff.rs`
- `crates/raios-surface-cli/src/cli/swarm.rs`
- `crates/raios-surface-cli/src/cli/action_types.rs`
- `crates/raios-surface-cli/src/cli/args.rs`
- `crates/raios-surface-cli/src/cli/mod.rs`

Docs/context:
- `README.md`
- `.github/copilot-instructions.md`
- `memory.md`

## Known Dirty Worktree Caveat

The repo already had unrelated dirty files before this implementation, especially:
- `crates/raios-core/src/db/control_plane.rs`
- `crates/raios-core/src/db/inbox_risk.rs`
- multiple `crates/raios-surface-tui/...` files

Please do not attribute those to Trace Memory unless a diff proves a direct dependency.

## Verification Already Run

Passed:

```bash
cargo test -p raios-core
cargo test -p raios-runtime
cargo test -p raios-surface-cli
cargo test -p raios-runtime evolution
cargo test -p raios-runtime trace_recall
cargo test -p raios-core tool_trace
cargo test -p raios-surface-cli trace
cargo test -p raios-surface-cli evolve
```

CLI smoke tests passed against temp DBs:

```bash
env XDG_CONFIG_HOME=/tmp/raios-trace-sprint3 \
  cargo run -q -p raios-surface-cli --bin raios -- \
  trace record --project R-AI-OS \
  --command 'cargo test -p raios-runtime trace_recall' \
  --error 'tool trace recall regression' \
  --fix 'attach trace memory to handoff and session review' \
  --tag trace --success

env XDG_CONFIG_HOME=/tmp/raios-trace-sprint3 \
  cargo run -q -p raios-surface-cli --bin raios -- \
  --json handoff --to codex-kaira --status success \
  --msg 'tool trace recall regression needs next implementation step' \
  --project /home/alaz/dev/core/R-AI-OS
```

Expected result observed:

```json
{
  "trace_memory_attached": true
}
```

Trace to evolution smoke:

```bash
env XDG_CONFIG_HOME=/tmp/raios-trace-evolve \
  cargo run -q -p raios-surface-cli --bin raios -- \
  --json evolve from-traces --project R-AI-OS -n 10
```

Expected result observed:

```json
{"inserted":1,"limit":10,"project":"R-AI-OS","status":"ok"}
```

KG export smoke:

```bash
env XDG_CONFIG_HOME=/tmp/raios-trace-evolve \
  cargo run -q -p raios-surface-cli --bin raios -- \
  --json trace kg-export trace --project R-AI-OS -n 2
```

Expected result observed:
- JSON array of KG facts.
- Includes predicates: `project`, `agent`, `command`, `success`, `observed_error`, `resolved_by`.

## Warnings Observed

Pre-existing warnings remain:
- unexpected cfg value `cortex`
- dead-code warnings around CLI preflight `AgentRunGate`/`run_gate`

These were not introduced by the Trace Memory behavior, but confirm before merge if warning-clean is required.

## Review Questions

1. Is `tool_trace_search` good enough with LIKE search for Sprint 1/2, or should this move to FTS5 before merge?
2. Is the trace content model too broad for plain SQLite, even with secret-like refusal?
3. Is `record_post_run_review_trace()` conservative enough to avoid memory pollution?
4. Is handoff enrichment safe after the second secret scan of the enriched message?
5. Is `CandidateStore` dual-schema support acceptable, or should the core/runtime schema split be resolved with a migration?
6. Is `kg-export` shape compatible enough with `mempalace_kg_add`, or should it output exact MCP call envelopes?

## Recommended Next Step

If review passes:
1. Commit trace memory implementation separately from unrelated TUI/control-plane dirty changes.
2. Run `cargo clippy --workspace -- -D warnings` only after deciding what to do with existing warnings.
3. If MemPalace automation is desired, add a dedicated MCP client/adapter instead of shelling out or silently writing from the CLI.
