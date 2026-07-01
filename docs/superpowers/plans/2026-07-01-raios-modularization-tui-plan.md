# R-AI-OS Modularization + TUI Refactor Plan

**Date:** 2026-07-01  
**Owner:** Codex Kaira  
**Status:** In Progress

## Goal

Split the current monolith into clearer layers without a big-bang rewrite:

1. `raios-core`
2. `raios-runtime`
3. `raios-surface-*`
4. `raios-module-*`

At the same time, move the TUI toward a pure-surface role:

- no direct `open_db()`
- no direct `cp_query_*`
- no shell/process orchestration inside view modules
- no business aggregation in panel render functions

## Target Architecture

### Core

Owns:

- control-plane data model
- policy / capability / sandbox / egress / audit
- auth / locks / provider capability registry
- canonical DB access

### Runtime

Owns:

- agent runner
- daemon
- swarm runtime
- scheduler / lifecycle workers
- session review and memory orchestration

### Surfaces

Own:

- CLI
- MCP
- HTTP / WebSocket
- TUI

Rule:

- surfaces call services
- surfaces do not aggregate raw DB rows themselves
- surfaces do not spawn commands directly unless explicitly a surface-only concern

### Optional Modules

Examples:

- A2A
- intelligence / search / cortex
- session review / stats / risk scoring
- VS Code extension
- tray

## TUI Refactor Principles

1. Every panel reads one typed snapshot, not raw DB queries.
2. Every panel snapshot comes from a service function or trait.
3. `app/events/*` becomes controller code, not business logic.
4. Background work and shell execution move behind runtime services.
5. TUI state and runtime-derived data stay separate.

## Migration Phases

### Phase 1 — TUI Service Boundary

Create typed service snapshots for high-risk panels:

1. Inbox
2. Scheduler
3. Policies
4. Projects

Deliverable:

- panel render code depends on service snapshots only

### Phase 2 — Controller Boundary

Move event-driven orchestration out of `ui/*` and large chunks of `app/*`:

- keyboard handlers call controller functions
- controller functions call services

Deliverable:

- `app/events/*` no longer performs direct DB reads for dashboard data

### Phase 3 — Runtime Extraction

Create runtime service façades for:

- inbox
- scheduler
- project status
- extension actions
- review/session summaries

Deliverable:

- reusable service layer callable from TUI, CLI, MCP, and HTTP

### Phase 4 — Crate Split Preparation

Once direct dependencies are reduced:

- move `core`
- move `runtime`
- move `surface-tui`

Deliverable:

- crate-ready file boundaries with minimal cross-layer leaks

## First Sprint Scope

### Selected Start

Started with `Inbox` because:

- it already contained direct DB access in a render path
- it was self-contained
- it established the panel snapshot pattern for later panels

### Sprint Tasks

1. Introduce `app/services.rs`
2. Add `InboxPanelData` snapshot
3. Move inbox DB reads into `load_inbox_panel_data()`
4. Update `ui/panels/inbox.rs` to render from snapshot only
5. Add service tests using in-memory DB
6. Extend the same service boundary to `Scheduler`, `Policies`, and `Projects`
7. Remove render-path `systemctl` probing from `Extensions` and precompute service status during discovery

## Follow-up Queue

After Inbox:

1. Scheduler snapshot service
2. Policies summary service
3. Project overview service
4. Extension command service

## Rules During Migration

- no behavior regressions
- additive refactor first, structural move later
- one panel at a time
- keep `cargo test --lib` and `cargo clippy --lib -- -D warnings` green after every slice

## Started Work

- 2026-07-01: `app/services.rs` created as the initial TUI service boundary
- 2026-07-01: `Inbox`, `Scheduler`, and `Policies` panels switched to typed snapshot/service loading
- 2026-07-01: `Projects` list view moved off inline aggregation onto `ProjectsPanelData`
- 2026-07-01: extension discovery now computes service status up front; `render_extensions` no longer calls `systemctl`
- 2026-07-01: `ProjectDetail` loading (`memory.md`, `git log`, `git diff`, `GRAPH_REPORT.md`) moved behind service helpers and typed payloads
- 2026-07-01: `execute_command()` shed inline helpers for daemon JSON command building and vault note creation; duplicate memo helper removed
- 2026-07-01: `ipc.rs` slimmed by extracting daemon bootstrap/auth/log helpers into `app/ipc_support.rs`
- 2026-07-01: `ipc.rs` event decoding split again: large daemon event `match` moved into dedicated `app/ipc_events.rs`
- 2026-07-01: `bg_messages.rs` split internally into focused lifecycle/sync/file-change/extension handler methods so `handle_bg_msg()` is now mostly routing
- 2026-07-01: verification after this slice: `cargo test --lib` = 451 passed, `cargo clippy --lib -- -D warnings` clean
- 2026-07-01: Refactored `app/events/keyboard/*` event handlers to move direct orchestrations (agent launching, daemon JSON messaging, async git commit/push threads, health refresh) behind `App` controller methods in `app/events/actions.rs`.
- 2026-07-01: Analysed import dependency boundaries for Core vs Runtime vs Surface split.
- 2026-07-01: Decoupled `src/workers.rs` from TUI `BgMsg` event type by defining a generic `RuntimeEvent` and forwarding events inside `App::new()`.
- 2026-07-01: Physically split the codebase into a Cargo workspace containing `raios-core`, `raios-runtime`, `raios-surface-tui`, `raios-surface-mcp`, and `raios-surface-cli`. Refactored all internal paths and resolved dependencies cleanly. Deleted root `src/` directory. Workspace verified clean compile and 452/452 tests pass.


