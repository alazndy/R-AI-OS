# Project Memory: R-AI-OS

## Context
- **Status**: In Development — Alpha Phase (v2.0.0-alpha "Aura Hardened Kernel")
- **Stack**: Rust 2021 Edition + Ratatui TUI + Tokio async + SQLite (rusqlite) + Axum HTTP + VS Code Extension
- **Last Milestone**: Control Plane Migration complete (all 8 phases) — 2026-06-10. 378 lib tests green.
- **Repo**: https://github.com/alazndy/R-AI-OS

## Active Objectives
- [ ] Phase 11: Tool Pinning — MCP tool manifest hash verification (detects supply chain tampering)
- [ ] Phase 12: Secret Leasing — `raios secret grant <tool> <ENV_VAR>` TTL-based env injection, auto-revoke
- [ ] Phase 2B: memory.md Write-Back — sidebar task state update via VS Code WebView
- [ ] Rate Limiting: Tool call frequency limiter (spam protection for AI loops)
- [ ] Quarantine Mode: Suspend suspicious agent calls, await human approval

## Technical Decisions
- **Architecture**: Modular kernel — `src/security/`, `src/kernel/`, `src/control_plane.rs`, `src/swarm/`, `src/cortex/`
- **Database**: SQLite via rusqlite (bundled). `cp_*` schema is the sole source of truth. Legacy tables are read-only cache.
- **Control Plane**: `cp_daemon_snapshot()` = single-call operational view. `cp_scheduler_list_ready()` = canonical task scheduler.
- **Security Kernel**: 4 layers — Filesystem Jail → Policy Manager → Verify Chain (hash-chained audit ledger) → Egress Filter
- **Policy**: `raios-policy.toml` is the single security config source. `confirm` = fail-closed (deny) in daemon/stdio mode.
- **Egress**: Filtered at MCP tool hook level (not OS network layer — scoped to what raios controls).
- **Hash Chain**: Tamper *detection*, not prevention — correctly classified as forensic tool.
- **IPC**: VS Code extension proxies requests to daemon (port 42071) via Bearer token — token never exposed to WebView.
- **Critical Notes**: New work must read/write through `cp_*` functions in `src/db.rs`. Never write directly to legacy tables.

## Important Links & Paths
- **Main Entry (CLI)**: `src/bin/raios.rs`
- **Daemon Entry**: `src/bin/aiosd.rs`
- **DB Layer**: `src/db.rs` — all `cp_*` functions here
- **Security**: `src/security/` — sandbox.rs, policy.rs, verify_chain.rs, egress.rs
- **Control Plane**: `src/control_plane.rs`
- **TUI**: `src/ui/panels/` (13 modules)
- **VS Code Extension**: `vscode-extension/`
- **Policy Config**: `raios-policy.toml`
- **Repo**: `gitrepo.md`

## Current Focus
- Start Phase 11 (Tool Pinning): hash the MCP tool manifest at startup, compare on each invocation, warn/block on drift.
- Reference: `src/mcp/` and `src/security/verify_chain.rs` for the pattern to follow.

## Change Log & Agent Trail
- 2026-06-03 [Antigravity Kaira]: Hash-Chained Audit Ledger, Redaction Engine, Sentry SDK integration — v2.0.0-alpha foundation
- 2026-06-04 [Antigravity Kaira]: Security Kernel 4 phases complete (Sandbox+Policy+Chain+Egress), 239 tests. Hybrid UI Faz 2A+2C (VS Code Sidebar WebView + TokenBridge). raios-0.4.0.vsix packaged.
- 2026-06-04 [Antigravity Kaira]: Refactor — events.rs→events/, dashboard.rs→13 panels, build.rs→10 submodules, deps.rs→10 submodules, keyboard.rs→6 submodules. False-positive risk pattern fix in refactor_scan.rs.
- 2026-06-10 [Claude Kaira]: Control Plane Migration phases 1-8 complete. cp_* is sole source of truth. Legacy tables cache-only. cp_daemon_snapshot() added. 376→378 tests green.
- 2026-06-13 [Claude Kaira]: memory.md migrated to AGENT_CONSTITUTION v5.0 template. PATH fixed (~/.cargo/bin added to .zshrc). Claude Kaira skills installed.
