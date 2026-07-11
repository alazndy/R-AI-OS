# Raios Trace Memory Revised Plan

## Purpose

This document is a revised implementation plan for adding MemOS-inspired value to `raios` without rebuilding capabilities that already exist in the current stack.

It is intentionally narrower than the original memory-engine proposal.

This revision assumes the current `raios` ecosystem already includes:

- Cortex for semantic search and memory-file indexing
- MemPalace for cross-session shared memory and KG facts
- `evolution.rs` / instinct infrastructure for candidate learning and promotion

The new work therefore focuses on the one remaining gap:

**cross-session, queryable tool-outcome + fix-context memory**

## Revised Goal

Add a lightweight Rust-native trace memory layer to `raios` that answers questions like:

- "How did we fix this before?"
- "What command sequence worked for this repo type?"
- "What usually resolves this build failure?"

This should integrate with existing systems instead of competing with them.

## Non-Goals

This revision explicitly does not attempt to build:

- a new general-purpose memory engine
- a new embedding subsystem
- a new knowledge/document store
- a new preference engine
- a new skill engine
- a parallel semantic search stack

Those capabilities already exist elsewhere in the current system and should be reused.

## Existing Systems To Reuse

### Cortex

Use existing Cortex capabilities for:

- semantic search
- indexing workspace files
- indexing memory files
- any future relevance ranking beyond basic trace lookup

### MemPalace

Use existing MemPalace capabilities for:

- cross-agent shared memory
- structured preference/profile facts
- long-term diary and project memory
- knowledge graph storage via subject-predicate-object facts

### Evolution / Instinct

Use existing instinct and evolution systems for:

- candidate learning
- human approval flow
- promotion of repeated high-value patterns

The new trace system should feed these where useful, not replace them.

## Core Product Direction

The new feature should be:

**Raios Trace Memory**

A small operational memory subsystem that records:

- command attempts
- failure context
- fix summaries
- eventual success or failure

and makes those records queryable later.

## Scope

### Include

- tool outcome recording
- failure/fix summaries
- queryable trace search
- deduplication
- secret redaction
- integration with `raios task`
- integration with `raios handoff`

### Exclude

- embeddings in v1
- document memory in v1
- preference extraction engine in v1
- skill candidate engine in v1
- cloud sync
- multimodal support

## Data Model

The data model should stay minimal.

### ToolTrace

Suggested fields:

- `id`
- `project`
- `agent`
- `command`
- `context`
- `outcome`
- `error_summary`
- `fix_summary`
- `tags_json`
- `success`
- `confidence`
- `created_at`
- `related_task_id`
- `related_handoff_id`
- `content_hash`

### TraceQuery

Suggested fields:

- `text`
- `project`
- `success_only`
- `tag`
- `limit`

## Storage Strategy

Use the existing SQLite approach already present in `raios`.

Do not introduce a second storage philosophy or a second search engine unless there is a demonstrated need.

Suggested new table:

- `tool_traces`

Possible schema fields:

- `id TEXT PRIMARY KEY`
- `project TEXT NOT NULL`
- `agent TEXT NOT NULL`
- `command TEXT NOT NULL`
- `context TEXT NOT NULL DEFAULT ''`
- `outcome TEXT NOT NULL DEFAULT ''`
- `error_summary TEXT NOT NULL DEFAULT ''`
- `fix_summary TEXT NOT NULL DEFAULT ''`
- `tags_json TEXT NOT NULL DEFAULT '[]'`
- `success INTEGER NOT NULL DEFAULT 0`
- `confidence REAL NOT NULL DEFAULT 0.5`
- `related_task_id TEXT`
- `related_handoff_id TEXT`
- `content_hash TEXT NOT NULL`
- `created_at TEXT NOT NULL`

Suggested indexes:

- `idx_tool_traces_project_created_at`
- `idx_tool_traces_success`
- `idx_tool_traces_content_hash`

## Abstraction Level

Keep abstractions small.

Recommended trait count:

- one small `ToolTraceStore` trait, or even no trait if the existing codebase patterns make a direct SQLite implementation more consistent

Illustrative shape:

```rust
pub trait ToolTraceStore {
    fn insert_trace(&self, trace: ToolTrace) -> Result<()>;
    fn search_traces(&self, query: TraceQuery) -> Result<Vec<ToolTrace>>;
    fn get_trace(&self, id: &str) -> Result<Option<ToolTrace>>;
    fn forget_trace(&self, id: &str) -> Result<()>;
}
```

Question for review:

Should even this trait exist, or should the feature follow the existing `cp_*`/DB module style directly without a trait?

## Recording Policy

Not every tool run should become a permanent trace.

### Good candidates for recording

- a failed command followed by a clearly identified fix
- a successful fix for a recurring operational error
- a known working recovery sequence
- a task-level or handoff-level resolution summary

### Bad candidates for recording

- raw secrets
- unredacted `.env` values
- noisy full log dumps
- low-signal one-off commands
- traces without useful context or resolution

## Redaction Requirements

This is mandatory from day one.

Redaction should mask:

- API keys
- bearer tokens
- passwords
- secret values
- environment variable secrets
- sensitive URL query parameters

The system should never persist raw secrets into trace memory.

## Deduplication Strategy

Prevent the trace store from becoming noisy.

Suggested rules:

- exact duplicate prevention via `content_hash`
- optional near-duplicate suppression based on:
  - same project
  - same command
  - same error summary
  - same fix summary

## Search Strategy

Keep v1 simple and consistent with the existing stack.

### v1

- SQLite-backed search
- simple ranking by:
  - project match
  - keyword hit
  - recency
  - success boost
  - confidence boost

### v2

Optionally integrate with Cortex ranking or semantic enrichment later if needed.

Do not start by building a separate retrieval stack.

## CLI Surface

The minimal useful command surface is:

- `raios trace record`
- `raios trace search "<query>"`
- `raios trace forget <id>`

Optional later additions:

- `raios trace show <id>`
- `raios trace list --project <name>`
- `raios trace suggest --task <id>`

## Example Command Shapes

### Record

```bash
raios trace record \
  --project gt-fit \
  --command "./gradlew :androidApp:compileDebugKotlin" \
  --error "Unresolved reference HealthConnectClient" \
  --fix "Added missing dependency and corrected import path" \
  --tag android \
  --tag gradle \
  --success
```

### Search

```bash
raios trace search "HealthConnect unresolved reference"
```

### Forget

```bash
raios trace forget <trace-id>
```

## Integration Plan

### `raios task`

Before or during task execution, surface relevant prior traces based on:

- project
- keywords
- repeated failure patterns

This should reduce repeated re-discovery work.

### `raios handoff`

When handing off work:

- link relevant trace IDs
- include short fix summaries where relevant
- allow the next agent to recall the prior operational context

### MemPalace KG Bridge

Do not build a new preference engine.

Instead:

- define rules for when a repeated or explicit preference becomes a KG fact
- write that fact into MemPalace using the existing KG structure

Example:

- `preferred_review_style = security_first`
- `preferred_chat_language = turkish`
- `preferred_code_language = english`

### Instinct / Evolution Bridge

Do not build a new skill engine.

Instead:

- if trace clusters reveal recurring successful patterns
- feed those into the existing instinct/evolution candidate pipeline

## Module Layout

Keep the implementation small and close to existing code patterns.

Suggested structure:

```text
crates/raios-core/src/db/tool_traces.rs
crates/raios-surface-cli/src/cli/trace.rs
```

Optional helper if needed:

```text
crates/raios-runtime/src/trace_recall.rs
```

No new crate unless the feature grows beyond this scope.

## Sprint Plan

## Sprint 1

Goal:

Make trace memory real and usable by hand.

Deliverables:

1. `tool_traces` table
2. insert/get/search/delete support
3. secret redaction
4. dedup via `content_hash`
5. CLI:
   - `raios trace record`
   - `raios trace search`
   - `raios trace forget`

Success criteria:

- a user can manually record a fix trace
- a user can find it again with search
- duplicate junk is suppressed
- secrets are not stored

## Sprint 2

Goal:

Integrate trace memory into actual `raios` workflows.

Deliverables:

1. `raios task` trace recall
2. `raios handoff` trace linking/surfacing
3. semi-automatic trace generation from:
   - failed command + later success
   - handoff/session summaries
   - explicit fix summaries
4. thin MemPalace KG bridge for confirmed preferences
5. optional bridge into instinct/evolution for repeated trace patterns

Success criteria:

- relevant past fixes are surfaced during tasks
- handoffs carry useful operational context
- preferences go into existing KG, not a parallel store
- repeated trace patterns can later inform instinct learning

## Risks

Main risks:

- trace spam
- poor-quality fix summaries
- secret leakage
- accidental duplication of existing systems
- over-automation too early

Mitigations:

- narrow command surface
- manual-first recording
- redaction
- dedup
- confidence scoring
- explicit reuse of Cortex/MemPalace/Evolution

## Main Decision

The main architectural decision is:

**Do not build a new memory platform. Build a narrow trace-memory layer that plugs into the memory systems `raios` already has.**

## Questions For Review

Please review this revised plan and answer:

1. Is the scope now correctly reduced?
2. Is `tool_traces` the right core abstraction?
3. Should this use a trait at all, or just follow current DB module style?
4. Is the proposed command surface minimal but sufficient?
5. Is `raios task` integration enough for Sprint 2, or should `handoff` be first?
6. Should trace search stay simple in v1, or should it immediately hook into Cortex ranking?
7. Are there obvious schema fields missing for future usefulness?
8. Is the MemPalace KG bridge the right way to handle preferences?
9. Is there any remaining hidden duplication risk with existing `raios` systems?
10. If this still needs to be smaller, what should be removed next?

## Requested Review Output

Please return:

- architecture critique
- scope validation or further reduction
- schema adjustments
- command UX adjustments
- sprint corrections
- integration risk notes

