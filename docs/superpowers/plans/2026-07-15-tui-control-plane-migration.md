# ADR & Architecture Blueprint: R-AI-OS TUI Control-Plane Migration

**Date:** 2026-07-15
**Author:** Antigravity Kaira
**Status:** Approved & Active Execution

---

## 1. Executive Summary & Context

R-AI-OS is evolving its TUI from an ad-hoc local SQLite + direct file I/O interface into a typed, attention-first control-plane client.
The migration keeps all existing surfaces (CLI, VS Code extension, MCP server, Rust/Ratatui TUI) while shifting full state authority to the resident `aiosd` daemon.

Surfaces emit **typed commands/queries** and consume **typed snapshot & delta events**. Render-time DB reads and magic integer menu indices (`menu_cursor == 0..=14`) are eliminated in favor of 4 primary typed routes: `Now`, `Work`, `Explore`, and `Govern`.

---

## 2. Baseline Inventory & Risk Profile

### Direct I/O Inventory (Render Paths & Controller Hooks)
1. **Render-Path DB Connections (`open_db` in `app/services.rs`)**:
   - `load_inbox_panel_data()` opens a new SQLite connection during UI render passes.
   - `load_scheduler_panel_data()` queries `cp_scheduled_jobs` directly in panel updates.
   - `load_policies_panel_data()` reads audit log row counts directly.
   - Project detail loading (`memory.md`, git log, git diff) occurs synchronously inside view/controller state.

2. **Direct Filesystem Access (`std::fs` in TUI modules)**:
   - `app/events/bg_messages.rs`: reads `memory.md`, vault dirs, and sentinel files on file change events.
   - `app/events/actions.rs`: direct `mtime` probing.
   - `setup_wizard`: direct template generation & file updates.

3. **Magic Menu Indexing (`menu_cursor: usize`)**:
   - Menu items were hardcoded 0..14 across key handlers (`keyboard/mod.rs`, `keyboard/editor.rs`, etc.).
   - Switching or adding panels caused hidden routing traps (e.g. Extensions panel focus trap).

### DB Growth Profile & Authority Trap
- Direct SQLite access from short-lived TUI threads risks lock contention (`SQLITE_BUSY`) against `aiosd` background workers (life cycle worker, search indexer, cron scheduler, audit ledger).
- Local DB access bypasses server-side security checks, audit logging, UMAI capability policy verification, and secret scanning on actions executed via TUI.

---

## 3. Crate Architecture Strategy

```
                          ┌──────────────────────────┐
                          │     raios-contracts      │ (Serialization-only DTOs)
                          └────────────┬─────────────┘
                                       │
                ┌──────────────────────┼──────────────────────┐
                ▼                      ▼                      ▼
    ┌───────────────────────┐ ┌───────────────────┐ ┌────────────────────┐
    │     raios-core        │ │   raios-runtime   │ │ raios-surface-cli  │
    └───────────────────────┘ └────────┬──────────┘ └────────────────────┘
                                       │
                         ┌─────────────┴─────────────┐
                         ▼                           ▼
            ┌─────────────────────────┐ ┌───────────────────────────┐
            │    raios-surface-mcp    │ │     raios-surface-tui     │
            └─────────────────────────┘ └───────────────────────────┘
```

1. **`raios-contracts`**:
   - Zero-dependency / serialization-only crate containing `Query`, `Command`, `Event`, `Problem`, `Snapshot`, and typed DTOs.
2. **`raios-runtime`**:
   - Daemon query/command router, control-plane projection engine, authorization, confirmation, idempotency cache, audit logger, and background workers.
3. **`raios-surface-tui`**:
   - Ratatui UI split into `client`, `store`, `route`, `intent`, `controller`, and `reducer` modules.
   - Driven entirely by daemon events/snapshots without direct SQLite handle allocation.

---

## 4. Route Specification: Attention-First TUI

The 14 legacy menu items are grouped into 4 high-level typed routes:

| Route | Sub-Views / Focus | Primary Purpose |
|---|---|---|
| **Now** | Approvals, Blockers, Active Runs, System Alerts | Emergency response & pending human confirmations |
| **Work** | Projects, Tasks, Active Agent Runs, Code Artifacts | Executive project navigation & session management |
| **Explore** | Trigram Search, Cortex Semantic Search, Tool Traces, Daemon Logs | System inspection, audit history & vector intelligence |
| **Govern** | Security Policies, UMAI Audit Ledger, System Health, Cron Scheduler | Safety controls, compliance, and automated task rules |

---

## 5. Migration Execution Order

1. **Phase 0**: Baseline inventory & ADR documentation (this document).
2. **Phase 1**: Introduce serialization-only `raios-contracts` crate and link to workspace.
3. **Phase 2**: Implement daemon control-plane services & snapshot projections in `raios-runtime`.
4. **Phase 3**: Rearchitect `raios-surface-tui` into typed `route`, `store`, `intent`, `controller`, `reducer`, and `client`.
5. **Phase 4**: Implement `Now`, `Work`, `Explore`, and `Govern` route screens.
6. **Phase 5**: Harden server-side command authorization, confirmation, idempotency, audit logging, and DB lifecycle.
7. **Phase 6**: Comprehensive test suite (Contracts serde, Reducer state, Golden render tests with Ratatui `TestBackend`, Reconnect resilience, and OWASP E2E checks).
