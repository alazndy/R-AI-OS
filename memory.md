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
- 2026-06-18 [Codex Kaira]: Cross-platform `raios-tray` project copy added under `tools/raios-tray/` in this repo, including PySide6 tray app, startup assets, and in-tray `aiosd` settings editor.
- 2026-06-22 [Claude Kaira]: Gemini CLI removed from entire codebase (~165 lines across 28+ source files) — Google shut down Gemini CLI. Removed WizardStep::Gemini, scan_gemini_usage, agent_runner gemini case, filebrowser Gemini CLI section, cli/new.rs config block, all gemini keybindings/actions from events/, tasks.rs agent parser patterns, discovery gemini paths, db.rs gemini provider, GEMINI.md hardlinks, ui panels references. Replaced gemini fallback with claude in cli/new.rs. Updated ~20 test files. 0 warnings, clean cargo check.
- 2026-06-25 [Claude Kaira]: `raios handoff` added — atomic 4-agent handoff (claude_kaira/codex_kaira/opencode_kaira/antigravity_kaira) built on the existing control plane, not a new STATE.json. `create_handoff_workflow`/`cp_take_pending_handoff`/`cp_consume_handoff` in db.rs reuse `cp_tasks`/`cp_agent_runs`/`cp_artifacts`/`cp_approvals` (`ArtifactKind::HandoverNote`, `ApprovalType::Handover` were already defined, now wired up). `agent_runner::run_agent` delivers pending handovers to `raios run`/`raios task` spawns via `RAIOS_HANDOVER_CONTEXT` env, exactly once. Also fixed a stale, never-committed `lock_manager.rs` test (`blocks_equal_priority_second_owner`) left broken by the 2026-06-22 gemini→claude rename above — it had started asserting against itself. 379 tests green, 0 warnings. AGENT_CONSTITUTION.md Section 1 + new Section 10 updated to match (OpenCode/Antigravity as separate identities, Gemini fully dropped).
- 2026-06-25 [Claude Kaira]: Live-tested the handoff above end-to-end against the real `~/.config/raios/workspace.db` (not a scratch DB): `raios handoff --to codex-kaira` then `raios run codex` delivered `[HANDOVER CONTEXT]` and flipped approval pending→approved, task awaiting_approval→completed. Surfaced a pre-existing gap while doing so: `agent_runner::run_agent`'s spawn match had no `"codex"` arm (`raios run codex` always errored "Unsupported agent" despite Codex Kaira being a canonical identity) — added it. 379 tests still green, 0 warnings.
- 2026-06-25 [Claude Kaira]: Found that `RAIOS_HANDOVER_CONTEXT` (and the pre-existing `RAIOS_INSTINCTS`) are env vars no CLI actually reads — `claude`/`codex`/`opencode` are real upstream binaries, not raios wrapper scripts. Rewired the handoff delivery in `agent_runner.rs` to use each tool's real prompt-injection surface: `claude --append-system-prompt <block>`, `codex <block>` (positional PROMPT), `opencode --prompt <block>`; env var kept only as a best-effort fallback for `antigravity` (not installed on this box, flag unverified). Confirmed via `strace -f -e trace=execve` that the handover text lands in `codex`'s actual argv. 379 tests green, 0 warnings.
