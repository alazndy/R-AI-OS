# R-AI-OS Control Plane Remaining Work Plan

## Purpose
This document is the execution plan for the work that remains after the current control-plane migration pass.

It is written for a follow-up coding agent, with the assumption that:

- the `cp_*` schema is now the source of truth for:
  - file approvals
  - swarm tasks
  - task graph execution
- legacy `task_graphs` and `task_graph_nodes` now act as a compatibility cache, not the primary state model
- the next objective is to extend the same model across the rest of the product surface and remove legacy orchestration assumptions

## What Is Already Done

### Canonical schema already in place
- `cp_tasks`
- `cp_agent_runs`
- `cp_artifacts`
- `cp_approvals`
- `cp_budget_ledger`
- `cp_task_edges`
- `cp_task_graph_nodes`
- `cp_task_graphs`
- `cp_task_list_items` (Phase 1)
- `cp_run_contracts` (Phase 4)
- `cp_provider_capabilities` (Phase 6)

### Flows already migrated
- file approval lifecycle
- swarm lifecycle
- task graph lifecycle
- task graph dependencies
- task graph read model
- task graph compatibility-cache rebuild path
- personal task management (Phase 1)
- unified inbox view / `cp_daemon_snapshot` (Phases 2, 8)
- canonical scheduler with budget + capability gating (Phases 3, 5, 6)
- run contracts as first-class stored objects (Phase 4)
- provider normalization + failure taxonomy (Phase 6)
- legacy cache drift detection + repair (Phase 7)

### Current behavioral reality — as of 2026-06-10
- all phases (1-8) are **complete**
- all task-like work is represented in `cp_*`
- personal tasks write through DB; `tasks.md` is a regenerable cache
- scheduler checks budget gates (Phase 5) and capability gates (Phase 6)
- `cp_daemon_snapshot` provides a single-call operational view for daemon/TUI (Phase 8)
- 376 lib tests green, clippy clean

## Remaining Work: High-Level Order

The remaining work should be done in this order:

1. Canonicalize personal task management
2. Build unified read models and inbox views from `cp_*`
3. Introduce a real scheduler over canonical tasks
4. Introduce run contracts as first-class stored objects
5. Wire budget enforcement into scheduler decisions
6. Normalize provider capability and failure handling
7. Add migration and pruning tools for legacy caches
8. Tighten docs, tests, and observability

Do not jump to UI polish before phases 1-5 are solid. The risk is ending up with multiple partial truths again.

---

## Phase 1: Canonicalize Personal Tasks

### Goal
Move the normal `tasks.md` / `tasks` flow onto `cp_tasks` so the system has one task model for both user-created work and agent-created work.

### Why this matters
Right now the control plane is strong for swarm and task graphs, but ordinary daily tasks still live in a separate legacy model. That means:

- no shared scheduler
- no shared inbox
- no shared status taxonomy
- no shared budgeting or approvals

### Scope

#### 1.1 Add a canonical mapping for plain tasks
Represent plain tasks as `cp_tasks` rows with:

- `plan_id = NULL`
- `parent_task_id = NULL`
- `assignee_kind = 'human' | 'agent' | NULL`
- `assignee_id` based on tagged agent if present

Map legacy task states:

- markdown unchecked -> `queued`
- markdown checked -> `completed`
- blocked keyword or future extension -> `blocked`

#### 1.2 Add a small metadata table if needed
If preserving markdown-specific fields becomes awkward, add a light metadata table, for example:

- `cp_task_list_items`
  - `task_id`
  - `source_kind`
  - `source_path`
  - `display_order`
  - `raw_text`
  - `created_at`

Do not overload `cp_tasks.description` with every markdown concern if that starts to distort the canonical model.

#### 1.3 Make `tasks.md` a cache/view, not source of truth
Rewrite load/save logic so:

- load path prefers canonical state
- save path regenerates `tasks.md`
- legacy `tasks` sqlite table becomes compat-only or is fully bypassed

### Files likely involved
- `src/tasks.rs`
- `src/db.rs`
- `src/control_plane.rs`
- `src/app/*` task list consumers
- `src/cli/*` task-related commands

### Acceptance criteria
- creating, toggling, listing tasks updates `cp_tasks`
- `tasks.md` can be deleted and rebuilt from canonical state
- no user-visible regression in CLI or TUI task listing
- tests cover rebuild and round-trip behavior

### Stop condition
Stop when normal personal tasks no longer depend on the legacy `tasks` table for truth.

---

## Phase 2: Unified Read Models And Inbox Views

### Goal
Expose one unified operational view over:

- personal tasks
- task graph nodes
- swarm tasks
- file approvals
- pending approvals
- completed outcomes

### Why this matters
Right now different flows are canonicalized internally, but the user still interacts with several separate views and concepts.

### Scope

#### 2.1 Add query helpers for unified views
Create DB helpers that can answer:

- all active tasks
- all pending approvals
- all agent runs in progress
- all blocked tasks
- all completed tasks by recency

These should come from `cp_*`, not legacy tables.

#### 2.2 Add view-model structs
Add normalized read structs, for example:

- `UnifiedTaskRow`
- `ApprovalInboxRow`
- `RunOverviewRow`

These are read models only. Do not make UI depend directly on raw DB rows.

#### 2.3 Move UI/CLI/MCP read paths to canonical queries
Priority order:

1. approval inbox
2. task list
3. swarm/task graph listings
4. daemon state snapshots

### Files likely involved
- `src/db.rs`
- `src/daemon/state.rs`
- `src/server/http.rs`
- `src/mcp/tools*.rs`
- `src/ui/*`
- `src/cli/*`

### Acceptance criteria
- a single query path can show active work regardless of origin
- approval inbox no longer depends on origin-specific structs alone
- MCP/HTTP can expose a unified task list

### Stop condition
Stop when “what work exists right now?” can be answered entirely from canonical read models.

---

## Phase 3: Canonical Scheduler

### Goal
Replace origin-specific execution decisions with one scheduler over `cp_tasks`.

### Why this matters
Canonical data without canonical scheduling still leaves orchestration fragmented.

### Scope

#### 3.1 Introduce scheduler selection rules
Scheduler should choose tasks based on:

- `status = ready`
- dependency satisfaction
- approval state
- lock conflicts
- provider availability
- budget availability
- priority

#### 3.2 Add task origin metadata if needed
Scheduler needs to know how to execute a task:

- task-graph shell node
- swarm branch task
- file approval task
- personal task

If current fields are insufficient, add explicit origin metadata, for example:

- `cp_task_execution`
  - `task_id`
  - `execution_kind`
  - `payload_json`

#### 3.3 Refactor daemon execution loops
Today task graph and swarm have their own logic.
Target:

- scheduler selects canonical task
- dispatcher routes to proper executor
- executor reports back into `cp_agent_runs`, `cp_artifacts`, `cp_approvals`

### Files likely involved
- `src/daemon/server.rs`
- `src/task_graph.rs`
- `src/swarm/*`
- `src/lock_manager.rs`
- `src/db.rs`

### Acceptance criteria
- scheduler can list next runnable work from canonical state
- task graph execution is routed through scheduler decisions, not isolated polling logic only
- swarm and future task types fit the same execution selection pattern

### Stop condition
Stop when the daemon can answer “what should run next?” from canonical scheduler logic alone.

---

## Phase 4: Run Contracts As First-Class Objects

### Goal
Store immutable run contracts instead of smuggling execution assumptions across ad-hoc call sites.

### Why this matters
Without a stored contract, auditability and deterministic execution are incomplete.

### Scope

#### 4.1 Add `cp_run_contracts`
Recommended fields:

- `id`
- `task_id`
- `workspace_root`
- `allowed_paths_json`
- `blocked_paths_json`
- `allowed_tools_json`
- `network_policy_json`
- `token_budget`
- `time_budget_secs`
- `cpu_budget_pct`
- `memory_budget_mb`
- `expected_artifacts_json`
- `success_criteria_json`
- `escalation_policy_json`
- `created_at`

#### 4.2 Create contracts for each execution kind
- task graph shell nodes
- swarm tasks
- file change approvals
- normal provider-backed tasks

#### 4.3 Use contract IDs everywhere
Agent runs should point to a real contract row, not a placeholder string.

### Files likely involved
- `src/db.rs`
- `src/control_plane.rs`
- `src/daemon/server.rs`
- `src/swarm/*`
- `src/task_graph.rs`

### Acceptance criteria
- every new `cp_agent_run` references a persisted run contract
- daemon can inspect a run and reconstruct its exact allowed scope and budgets

### Stop condition
Stop when a task run can be audited without inferring hidden execution rules from code.

---

## Phase 5: Budget Enforcement

### Goal
Make `cp_budget_ledger` operational instead of informational.

### Why this matters
The system already knows usage-related information, but the scheduler does not yet govern behavior from it.

### Scope

#### 5.1 Define budget scopes
At minimum:

- provider-wide
- project-wide
- task-wide
- run-wide

#### 5.2 Enforce soft and hard gates
Examples:

- do not start high-cost run if provider budget is exhausted
- limit concurrent agent runs when CPU pressure is high
- pause optional indexing under pressure
- downgrade to safer provider or cheaper path when budgets are constrained

#### 5.3 Distinguish confidence
Respect:

- `exact`
- `estimated`
- `unavailable`

Do not hard-block on unreliable data unless policy explicitly says so.

### Files likely involved
- `src/db.rs`
- `src/system_scan.rs`
- `src/daemon/server.rs`
- `src/config.rs`
- `src/ui/health.rs`

### Acceptance criteria
- scheduler consults budget state before starting work
- user can see why a task is deferred for budget reasons
- budget decisions are auditable

### Stop condition
Stop when budget data can actually delay, route, or reject work.

---

## Phase 6: Provider Normalization

### Goal
Normalize provider differences so scheduling and execution decisions stop scattering across provider-specific assumptions.

### Scope

#### 6.1 Add capability vocabulary
Per provider:

- supports tool calling
- supports patch/diff workflows
- supports long-running sessions
- supports streaming
- supports exact quota visibility

#### 6.2 Add failure taxonomy
Normalize failures into categories:

- auth
- quota
- timeout
- sandbox
- tool_error
- human_rejection
- provider_unavailable

#### 6.3 Route by capability and policy
Scheduler should use this model to choose or reject execution backends.

### Acceptance criteria
- daemon can reason about providers without provider-specific branching everywhere
- failed runs have normalized exit categories

---

## Phase 7: Migration, Pruning, And Legacy Controls

### Goal
Make legacy compatibility explicit and safe.

### Scope

#### 7.1 Mark legacy tables as cache in docs and code comments
This prevents future regressions where new code accidentally treats them as source of truth.

#### 7.2 Add explicit repair/rebuild commands
Useful examples:

- rebuild task graph cache from control plane
- rebuild personal task markdown from control plane
- validate canonical vs cache divergence

#### 7.3 Add drift detection
If cache rows diverge from canonical state, detect and heal.

### Acceptance criteria
- there is a deterministic path to rebuild legacy cache state
- legacy divergence is detectable in tests or diagnostics

---

## Phase 8: Tests, Docs, And Observability

### Goal
Make the new model maintainable for future agents and contributors.

### Scope

#### 8.1 Add integration tests per flow
- file approval
- swarm lifecycle
- task graph lifecycle
- personal task lifecycle
- unified inbox view

#### 8.2 Add daemon snapshot visibility
Daemon state snapshots should expose:

- active canonical tasks
- active runs
- pending approvals
- blocked reasons
- budget deferrals

#### 8.3 Update docs
Update:

- wiki
- control plane blueprint
- CLI reference
- any architecture notes still implying legacy truth

---

## Suggested Execution Order For Claude

Claude should apply the remaining work in this exact order:

1. Phase 1
2. Phase 2
3. Phase 3
4. Phase 4
5. Phase 5
6. Phase 7
7. Phase 6
8. Phase 8

Reason:
- phases 1-5 establish the real operating model
- phase 7 makes legacy safe after the new truth is stable
- phase 6 depends on scheduler and budget hooks being real
- phase 8 should document and harden the final shape

## Hard Rules For The Follow-Up Agent

- Do not reintroduce new sources of truth outside `cp_*`.
- Do not let UI convenience structs become persistent state models.
- Do not block on perfect schema elegance; prefer one-directional migration and explicit compat layers.
- Keep legacy tables readable until replacement views are proven.
- After every phase:
  - run `cargo test`
  - run `cargo clippy --lib -- -W clippy::all`

## Definition Of Done

The control-plane migration is functionally complete when:

- all task-like work is represented in `cp_tasks`
- all execution attempts are represented in `cp_agent_runs`
- all reviewable outputs are represented in `cp_artifacts`
- all human gates are represented in `cp_approvals`
- all dependency decisions come from canonical dependency data
- scheduler decisions come from canonical state
- legacy task tables are cache or removed
- UI/CLI/MCP/HTTP read models are built from canonical queries

Until then, the migration is still in progress even if individual flows already work.
