# Project Memory: R-AI-OS

## Context
- **Status**: In Development — Alpha Phase (v2.0.0-alpha "Aura Hardened Kernel")
- **Stack**: Rust 2021 Edition + Ratatui TUI + Tokio async + SQLite (rusqlite) + Axum HTTP + VS Code Extension
- **Last Milestone**: Lifecycle Worker + ZRAM system optimization — 2026-06-16. 378 lib tests green.
- **Repo**: https://github.com/alazndy/R-AI-OS

## Active Objectives
- [x] Phase 11: Tool Pinning — implemented & wired into MCP dispatch (`security/tool_pin.rs`)
- [x] Phase 12: Secret Leasing — `raios secret grant/list/revoke` fully implemented
- [x] Phase 2B: memory.md Write-Back — sidebar checkboxes now interactive; `raios task-update` CLI added
- [x] Rate Limiting — `security/rate_limiter.rs`, configurable via raios-policy.toml
- [x] Quarantine Mode — `security/quarantine.rs`, MCP dispatch integration complete
- [x] Lifecycle Worker — auto beklemede/archived/active transitions based on git activity (`daemon/lifecycle.rs`)
- [x] discover status-preserve fix — beklemede/archived no longer reset by discover
- [ ] aiosd systemd service — auto-start on login (currently manual start)

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
- All planned security phases complete. Project in maintenance/hardening mode.
- Next: aiosd systemd user service for auto-start, VS Code extension package bump.

## Change Log & Agent Trail
- 2026-06-03 [Antigravity Kaira]: Hash-Chained Audit Ledger, Redaction Engine, Sentry SDK integration — v2.0.0-alpha foundation
- 2026-06-04 [Antigravity Kaira]: Security Kernel 4 phases complete (Sandbox+Policy+Chain+Egress), 239 tests. Hybrid UI Faz 2A+2C (VS Code Sidebar WebView + TokenBridge). raios-0.4.0.vsix packaged.
- 2026-06-04 [Antigravity Kaira]: Refactor — events.rs→events/, dashboard.rs→13 panels, build.rs→10 submodules, deps.rs→10 submodules, keyboard.rs→6 submodules. False-positive risk pattern fix in refactor_scan.rs.
- 2026-06-10 [Claude Kaira]: Control Plane Migration phases 1-8 complete. cp_* is sole source of truth. Legacy tables cache-only. cp_daemon_snapshot() added. 376→378 tests green.
- 2026-06-13 [Claude Kaira]: memory.md migrated to AGENT_CONSTITUTION v5.0 template. PATH fixed (~/.cargo/bin added to .zshrc). Claude Kaira skills installed.
- 2026-06-13 [Claude Kaira]: K-AI-RA identity integrated into setup wizard. Project dedup fix (145→52). DB vacuum (88MB→308KB). Zombie aiosd fix. raios-tray built (AppIndicator3, daemon toggle, all-projects popup, agent launcher). Phase 2B write-back implemented (sidebar checkboxes + raios task-update CLI). All backlog phases confirmed complete. 378 tests green.
- 2026-06-16 [Claude Kaira]: Lifecycle worker added (auto beklemede/archived/active via git activity). DaemonConfig: lifecycle_standby_days/archive_days/interval_secs. upsert_project CASE fix (beklemede/archived preserved on discover). ZRAM 8GB + swappiness=10 system optimization. aiosd config intervals increased (CPU relief). Active project set defined in DB.
