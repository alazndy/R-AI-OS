# R-AI-OS Control Plane Blueprint

## 1. Purpose
This blueprint defines the minimum control-plane architecture required for R-AI-OS to evolve from a secure multi-tool kernel into a reliable **project tracker + agent harness**.

The core claim is simple:

> R-AI-OS should manage work as typed state, not as loose chat, shell output, or ad-hoc events.

Without a canonical state model, deterministic run contracts, and a scheduler that owns task execution, the system will remain feature-rich but operationally fragile.

## 2. North Star
R-AI-OS should become the single authority for:

- what work exists
- who owns it
- what inputs and budgets apply
- which artifacts were produced
- which approvals are pending
- what counts as success or failure

In other words: the daemon should own **workflow truth**, while agents become interchangeable execution backends.

## 3. Design Principles

### 3.1 State First
Every important object must exist as a typed entity with explicit lifecycle transitions.

### 3.2 Artifact First
Agent output is not trusted unless it resolves into typed artifacts:
- patch
- diff
- file change request
- build result
- test result
- report
- handover note

### 3.3 Deterministic Contracts
Agents should receive a fixed execution contract:
- scope
- tools
- budget
- success criteria
- expected artifact kinds

### 3.4 Control Plane Owns Orchestration
Agents do not schedule themselves. The scheduler decides:
- when work starts
- when it pauses
- when it retries
- when it escalates
- when it requires approval

### 3.5 Explicit Confidence
Any provider, quota, or runtime metric exposed by the system must say whether it is:
- exact
- estimated
- unavailable

## 4. Required Domain Model

The following entities should become first-class objects in the daemon, database, MCP, HTTP, and TUI.

### 4.1 Project
Represents a tracked codebase or workspace root.

Required fields:
- `id`
- `name`
- `path`
- `category`
- `status`
- `default_branch`
- `build_type`
- `health_summary`
- `memory_path`
- `created_at`
- `updated_at`

Project states:
- `active`
- `archived`
- `paused`
- `broken`

### 4.2 Plan
Represents a user-approved or system-generated work plan for a project.

Required fields:
- `id`
- `project_id`
- `title`
- `goal`
- `source`
- `status`
- `created_by`
- `approved_by`
- `created_at`
- `updated_at`

Plan states:
- `draft`
- `ready`
- `approved`
- `running`
- `completed`
- `cancelled`

### 4.3 Task
Represents a schedulable unit of work.

Required fields:
- `id`
- `project_id`
- `plan_id`
- `parent_task_id`
- `title`
- `description`
- `priority`
- `status`
- `assignee_kind`
- `assignee_id`
- `acceptance_criteria`
- `depends_on`
- `created_at`
- `updated_at`

Task states:
- `queued`
- `ready`
- `running`
- `blocked`
- `awaiting_approval`
- `completed`
- `failed`
- `cancelled`

### 4.4 Agent Run
Represents one concrete execution attempt of a task by one provider/agent.

Required fields:
- `id`
- `task_id`
- `project_id`
- `provider`
- `agent_name`
- `run_contract_id`
- `attempt`
- `status`
- `started_at`
- `ended_at`
- `exit_reason`
- `summary`

Agent run states:
- `pending`
- `starting`
- `running`
- `awaiting_input`
- `awaiting_approval`
- `succeeded`
- `failed`
- `timed_out`
- `cancelled`

### 4.5 Run Contract
Represents the immutable rules an agent run executes under.

Required fields:
- `id`
- `task_id`
- `workspace_root`
- `allowed_paths`
- `blocked_paths`
- `allowed_tools`
- `network_policy`
- `token_budget`
- `time_budget_secs`
- `cpu_budget_pct`
- `memory_budget_mb`
- `expected_artifacts`
- `success_criteria`
- `escalation_policy`

This object is the heart of deterministic execution.

### 4.6 Artifact
Represents a produced, reviewable output.

Required fields:
- `id`
- `task_id`
- `agent_run_id`
- `kind`
- `status`
- `path`
- `content_ref`
- `metadata_json`
- `created_at`

Artifact kinds:
- `patch`
- `diff`
- `file_change`
- `build_log`
- `test_report`
- `security_report`
- `research_note`
- `handover_note`
- `session_note`

Artifact states:
- `draft`
- `submitted`
- `approved`
- `rejected`
- `applied`
- `superseded`

### 4.7 Approval
Represents a human decision point.

Required fields:
- `id`
- `project_id`
- `task_id`
- `agent_run_id`
- `artifact_id`
- `approval_type`
- `reason`
- `status`
- `requested_at`
- `resolved_at`
- `resolved_by`

Approval types:
- `file_write`
- `merge`
- `handover`
- `network_exception`
- `tool_quarantine`
- `budget_override`

Approval states:
- `pending`
- `approved`
- `rejected`
- `expired`

### 4.8 Budget Ledger
Tracks consumption across project, provider, and run.

Required fields:
- `id`
- `scope_kind`
- `scope_id`
- `provider`
- `metric`
- `limit_value`
- `used_value`
- `remaining_value`
- `reset_at`
- `confidence`
- `source`
- `observed_at`

Metrics:
- `tokens`
- `requests`
- `cpu_seconds`
- `memory_mb`
- `wall_clock_secs`

### 4.9 Outcome
Represents the terminal evaluation of a task or plan.

Required fields:
- `id`
- `scope_kind`
- `scope_id`
- `result`
- `reason`
- `score`
- `artifact_refs`
- `recorded_at`

Outcome values:
- `success`
- `partial_success`
- `failure`
- `blocked`
- `aborted`

## 5. State Machines

### 5.1 Task State Machine
Valid transitions:

- `queued -> ready`
- `ready -> running`
- `running -> awaiting_approval`
- `running -> blocked`
- `running -> completed`
- `running -> failed`
- `awaiting_approval -> running`
- `awaiting_approval -> failed`
- `blocked -> ready`
- `blocked -> cancelled`

Invalid transitions:
- `completed -> running`
- `failed -> running` without creating a new `AgentRun`

Rule:
A retry must create a new `AgentRun`, not mutate history.

### 5.2 Agent Run State Machine
Valid transitions:

- `pending -> starting`
- `starting -> running`
- `running -> awaiting_input`
- `running -> awaiting_approval`
- `running -> succeeded`
- `running -> failed`
- `running -> timed_out`
- `awaiting_approval -> running`
- `awaiting_input -> running`

Rule:
An agent run is append-only history. Never recycle a failed run ID.

### 5.3 Artifact State Machine
Valid transitions:

- `draft -> submitted`
- `submitted -> approved`
- `submitted -> rejected`
- `approved -> applied`
- `approved -> superseded`

Rule:
Only approved artifacts can mutate project state.

## 6. Deterministic Execution Contract

Before an agent starts, the control plane should materialize a run contract like:

```json
{
  "task_id": "task_123",
  "project_id": "proj_abc",
  "workspace_root": "/repo",
  "allowed_paths": ["/repo/src", "/repo/tests"],
  "blocked_paths": ["/repo/.git", "/repo/secrets"],
  "allowed_tools": ["search", "read", "edit", "build", "test"],
  "network_policy": "deny_by_default",
  "token_budget": 120000,
  "time_budget_secs": 1800,
  "cpu_budget_pct": 35,
  "memory_budget_mb": 1024,
  "expected_artifacts": ["patch", "test_report"],
  "success_criteria": [
    "tests pass",
    "no sandbox violations",
    "patch applies cleanly"
  ],
  "escalation_policy": {
    "requires_human_for": ["file_write", "merge", "budget_override"]
  }
}
```

This object should be visible in:
- daemon state
- DB
- MCP tool responses
- HTTP API
- TUI detail view

## 7. Scheduler Requirements

The current system already has task graph primitives. The next step is turning them into the canonical scheduler.

The scheduler must own:

### 7.1 Readiness
A task is runnable only if:
- dependencies are completed
- no conflicting lock exists
- budget is available
- required provider is available
- project is not paused

### 7.2 Concurrency
Parallelism is allowed only when:
- task dependency graph permits it
- file/path scopes do not overlap destructively
- provider and system budgets allow it

### 7.3 Retry Policy
Retry should be explicit:
- `max_attempts`
- `retry_on`
- `backoff_secs`
- `escalate_after`

### 7.4 Escalation
The scheduler should escalate when:
- repeated sandbox violation
- repeated build failure with same signature
- quota exhausted
- human approval timeout
- no available provider can satisfy contract

### 7.5 Fairness
Avoid starvation:
- no single provider should monopolize the queue
- low-priority tasks should not block critical approvals

## 8. Artifact Pipeline

The control plane should standardize the path from agent output to project mutation:

1. Agent run produces raw output.
2. Output is normalized into typed artifacts.
3. Artifacts are validated.
4. Risky artifacts generate approvals.
5. Approved artifacts are applied.
6. Applied artifacts update task and project state.
7. Outcome is recorded.

Raw chat or shell output should never be treated as authoritative project mutation.

## 9. Approval Pipeline

Approvals should be unified across all risky actions.

Current patterns already exist for:
- file changes
- handovers
- quarantine

These should converge to one approval model with:
- one table
- one TUI inbox
- one HTTP feed
- one MCP representation

Approval decision inputs:
- artifact preview
- originating task
- agent/provider
- risk reason
- affected paths
- budget impact

## 10. Budget and Load Governor

R-AI-OS cannot become a dependable harness without runtime self-control.

The governor should monitor:
- active agent count
- CPU load
- RAM usage
- background worker pressure
- indexing pressure
- provider quota state

Decisions the governor must support:
- delay task dispatch
- reduce concurrency
- skip expensive indexing
- block non-critical scans
- prefer cheaper provider
- require human override

Policy examples:
- if CPU > 75%, stop launching new background runs
- if provider remaining quota is low-confidence or exhausted, route to fallback
- if memory exceeds threshold, suspend semantic indexing

## 11. Provider Abstraction

Providers should implement one common contract.

### 11.1 Provider Capabilities
Each provider descriptor should answer:
- can it edit files
- can it run shell
- can it call tools
- can it stream
- can it return structured output
- does it expose exact quota telemetry

### 11.2 Provider Status
Each provider should expose:
- `installed`
- `authenticated`
- `plan`
- `quota_confidence`
- `auth_expires_at`
- `reset_at`
- `health`

### 11.3 Normalized Failure Taxonomy
All provider failures should map to:
- `auth_error`
- `quota_exhausted`
- `network_error`
- `tool_error`
- `sandbox_error`
- `contract_error`
- `unknown_error`

Without this abstraction, scheduler logic becomes provider-specific and brittle.

## 12. Storage Model

SQLite remains the right default control-plane store.

Recommended tables:

- `projects`
- `plans`
- `tasks`
- `task_dependencies`
- `agent_runs`
- `run_contracts`
- `artifacts`
- `approvals`
- `budget_ledger`
- `outcomes`
- `provider_status`
- `run_events`

### 12.1 Minimal Schema Direction

`projects`
- `id TEXT PRIMARY KEY`
- `name TEXT`
- `path TEXT UNIQUE`
- `status TEXT`
- `category TEXT`
- `build_type TEXT`
- `created_at TEXT`
- `updated_at TEXT`

`tasks`
- `id TEXT PRIMARY KEY`
- `project_id TEXT NOT NULL`
- `plan_id TEXT`
- `parent_task_id TEXT`
- `title TEXT NOT NULL`
- `description TEXT NOT NULL`
- `priority INTEGER NOT NULL`
- `status TEXT NOT NULL`
- `assignee_kind TEXT`
- `assignee_id TEXT`
- `acceptance_criteria TEXT`
- `created_at TEXT`
- `updated_at TEXT`

`agent_runs`
- `id TEXT PRIMARY KEY`
- `task_id TEXT NOT NULL`
- `project_id TEXT NOT NULL`
- `provider TEXT NOT NULL`
- `agent_name TEXT NOT NULL`
- `run_contract_id TEXT NOT NULL`
- `attempt INTEGER NOT NULL`
- `status TEXT NOT NULL`
- `started_at TEXT`
- `ended_at TEXT`
- `exit_reason TEXT`
- `summary TEXT`

`artifacts`
- `id TEXT PRIMARY KEY`
- `task_id TEXT NOT NULL`
- `agent_run_id TEXT NOT NULL`
- `kind TEXT NOT NULL`
- `status TEXT NOT NULL`
- `path TEXT`
- `content_ref TEXT`
- `metadata_json TEXT`
- `created_at TEXT NOT NULL`

`approvals`
- `id TEXT PRIMARY KEY`
- `project_id TEXT NOT NULL`
- `task_id TEXT`
- `agent_run_id TEXT`
- `artifact_id TEXT`
- `approval_type TEXT NOT NULL`
- `reason TEXT NOT NULL`
- `status TEXT NOT NULL`
- `requested_at TEXT NOT NULL`
- `resolved_at TEXT`
- `resolved_by TEXT`

`budget_ledger`
- `id TEXT PRIMARY KEY`
- `scope_kind TEXT NOT NULL`
- `scope_id TEXT NOT NULL`
- `provider TEXT`
- `metric TEXT NOT NULL`
- `limit_value REAL`
- `used_value REAL`
- `remaining_value REAL`
- `reset_at TEXT`
- `confidence TEXT NOT NULL`
- `source TEXT NOT NULL`
- `observed_at TEXT NOT NULL`

## 13. Protocol Surface Mapping

### 13.1 Daemon TCP
Best for:
- run lifecycle events
- scheduler decisions
- approval notifications
- background state sync

Should become the primary event bus for control-plane truth.

### 13.2 MCP
Best for:
- querying project/task/run state
- creating plans/tasks
- submitting artifacts
- reading approval queues
- checking provider/budget status

MCP tools should stop being a loose bag of utilities and start exposing typed control-plane operations.

### 13.3 HTTP / WebSocket
Best for:
- dashboard views
- external integrations
- IDE status panels
- live event streams

Recommended new endpoints:
- `GET /api/projects/:id`
- `GET /api/tasks`
- `GET /api/tasks/:id`
- `GET /api/runs/:id`
- `GET /api/approvals`
- `GET /api/budgets`
- `POST /api/tasks`
- `POST /api/tasks/:id/retry`
- `POST /api/approvals/:id/approve`
- `POST /api/approvals/:id/reject`

### 13.4 TUI
The TUI should pivot from panel-first UX to workflow-first UX:
- project list
- plan view
- task board
- run detail
- approval inbox
- budget/provider health

## 14. Golden Path UX

The main workflow should be:

1. Select project.
2. Create or accept plan.
3. Break plan into typed tasks.
4. Scheduler dispatches ready tasks.
5. Agents produce artifacts.
6. User reviews approvals only where needed.
7. System validates build/test/security.
8. Merge/apply.
9. Memory, session notes, and outcomes are recorded automatically.

If this path is not smooth, the system will remain powerful but hard to trust.

## 15. Migration Strategy

This should not be a rewrite. It should be a staged convergence.

### Phase A: Canonical Entity Layer
Add new DB tables and Rust structs for:
- `Task`
- `AgentRun`
- `Artifact`
- `Approval`
- `BudgetLedger`

Do not remove existing flows yet.

### Phase B: Scheduler Adoption
Move existing:
- swarm tasks
- task graph nodes
- file approvals
- handovers

onto the canonical scheduler and approval model.

### Phase C: Provider Normalization
Unify Codex, Claude, Gemini, and Antigravity launchers under one provider trait.

### Phase D: UX Convergence
Expose the same typed state in:
- TUI
- MCP
- HTTP
- CLI

### Phase E: Policy Hardening
Make run contracts mandatory for all agent execution paths.

## 16. What Must Not Happen

The following anti-patterns should be treated as architectural regression:

- direct project mutation without artifact creation
- retrying a failed run by mutating the same run record
- provider-specific scheduler branches spread across the codebase
- approvals implemented as special-case queues per feature
- background workers consuming unbounded CPU without budget coordination
- tool outputs being treated as workflow truth without normalization

## 17. Immediate Next Steps

The next concrete implementation sequence should be:

1. Introduce canonical Rust structs for `Task`, `AgentRun`, `Artifact`, `Approval`, and `BudgetLedger`.
2. Add SQLite tables and migration path.
3. Wrap current file-change approval flow in canonical `Artifact + Approval`.
4. Wrap current swarm flow in canonical `Task + AgentRun + Artifact`.
5. Add one scheduler service in daemon that owns readiness and retry.
6. Add one provider trait for Codex, Claude, Gemini, Antigravity.
7. Move TUI and MCP views to read from canonical state instead of feature-specific stores.

## 18. Decision Summary

R-AI-OS does **not** primarily need more tools.

It needs:
- a canonical domain model
- deterministic run contracts
- a real scheduler
- an artifact pipeline
- a unified approval model
- a load and budget governor

If these are implemented first, the system can grow into a dependable agent harness without collapsing under its own complexity.
