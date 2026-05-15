# R-AI-OS Memory

## Current Status
- Date: 2026-05-15
- Active agent: Claude (v1.3.0 — AI Intelligence Layer tamamlandı)
- Version: v1.3.0
- Version Name: AI Intelligence Layer
- Status: **Production-ready.** 35 CLI commands, 23 MCP tools, 75 unit tests — all green. 3 yeni özellik: Hybrid Memory Search, Sentinel Guard Watch, Instinct Automation.

## Claude
### Achievements
- **Phase 1A: SQLite Migration:** `entities.json` structure fully migrated to `rusqlite` based SQLite database.
- **Phase 1B: Manifest System:** `.raios.yaml` manifest support added.
- **Phase 2: Embedded Workers:** Standalone operation achieved without daemon via `workers.rs` module.
- **Phase 3A: Event-driven Sentinel:** Switched to event-driven structure with `notify` library.
- **Phase 4: Security & Testing:** Semgrep + 22 unit tests.
- **Refactor Scanner:** `src/refactor_scan.rs` — detection of line count, unwrap chains, nesting depth. REFACTOR column added to health view, warning added to dashboard.
- **Core Toolkit (v1.2.0):** `src/core/` layer — 7 modules, all accessible via CLI + MCP tool:
  - `git.rs` — status, log, diff, commit, push, pull, branch, checkout (9 CLI, 4 MCP tools)
  - `build.rs` — Rust/Node/Python/Go build + test runner (2 CLI, 2 MCP tools)
  - `deps.rs` — outdated + CVE scanning, cargo audit / npm audit (1 CLI, 1 MCP tool)
  - `env.rs` — .env vs .env.example diff, missing/empty key detection (1 CLI, 1 MCP tool)
  - `version.rs` — semver bump, CHANGELOG.md generation, git tag (2 CLI, 2 MCP tools)
  - `process.rs` — port list, process list, kill-port (2 CLI, 1 MCP tool)
  - `disk.rs` — project size analysis, cache cleaning (2 CLI, 1 MCP tool)
- **Aggregate MCP Tools:** `project_info` (git+health+version+env+disk in one call) + `portfolio_status` (summary table for 42 projects). Reduced 5-8 tool calls to 1.
- **TUI Enhancements:** Project detail panel shows health grades + constitution issues + env flags. Added `[c]` commit / `[p]` push shortcuts in health view.
- **E2E Test:** 66/66 unit tests green. CLI smoke tests passed. 23 MCP tools verified.
- **Total:** 32 CLI commands, 23 MCP tools, 66 unit tests
- **v1.3.0 — AI Intelligence Layer (2026-05-15):**
  - **Faz 1 — Hybrid Memory Search:** `raios memory --query "<text>" --top N` — tüm projelerin memory/AGENTS/MASTER/CLAUDE.md dosyalarında semantic arama. `Cortex`'e `search_with_filter()` + `index_memory_files()` + `MEMORY_PATTERNS` eklendi. Auto-index, JSON çıktı desteği. OnceLock ile regex önbelleği.
  - **Faz 2 — Sentinel Guard Watch:** `raios security [--watch] [--json]` — tek seferlik OWASP taraması veya sürekli dosya izleme. `notify-rust` ile Windows toast bildirimi. `scan_file()` + `WATCHED_EXTS` + `compiled_pattern_regexes()` (OnceLock). 11 uzantı izleniyor.
  - **Faz 3 — Instinct Automation:** `raios instinct add/list/suggest` — manuel + otomatik instinct yönetimi. `suggest_from_health()` 6 pattern analizi. `append_to_memory_md()` duplicate-safe. Global `~/.agents/instincts.json` + per-project `memory.md ## Instincts`. Health footer.
  - **Toplam:** 35 CLI commands, 75 unit tests, 14 yeni commit.

## Gemini
### Achievements
- **SIGMAP Tracking:** `has_sigmap` columns added to R-AI-OS Health Dashboard and SQLite (`health_cache`) database to centrally track the `sigmap` status of all projects.
- **Project Versioning:** Added support for automatic version and nickname tracking via `memory.md`.
- **Self-Healing Loop:** Added `ValidationWorker` to `aiosd`. `cargo check` and compliance results can be reported via MCP.
- **Architectural Memory:** Added RAG-based architectural consultancy layer with `ask_architect` MCP tool.
- **Workspace Sync:** MASTER.md and paths updated according to `Dev_Ops_New` structure.
- **UI Performance Fix:** Lag in the All Projects screen resolved by removing synchronous I/O and using cache.
- **Visual Grid Refactor:** All Projects screen upgraded to a modern `Table` structure.

## Antigravity
### Achievements
- **Table-Based Health UI:** Dashboard list refreshed with `ratatui::Table`.
- **Binary Recovery:** File locking issues on Windows resolved via process management.
- **Refactor (High Priority):** 
  - Duplicate code (dead code) in `app/events.rs` cleaned up.
  - Async panic risk in `daemon/server.rs` (`Cortex::init().unwrap()`) resolved (error handling added).
  - Shell command injection vulnerability in `run_graphify` method in `app/mod.rs` resolved.
  - O(n²) filesystem I/O operations in project sorting in `app/mod.rs` prevented and switched to reading from cache.
- **Events Monolith Modularization:** `src/app/events.rs` (1700+ lines) broken down and moved to `src/app/events/` module. Separated into `actions`, `bg_messages`, `commands`, `keyboard`, and `helpers`. SRP compliance achieved.
- **UI Component Extraction:** `src/ui/dashboard.rs` (900+ lines) broken down into 13 separate modules under `src/ui/panels/`. Dashboard orchestration made component-based.
- **Clippy Cleanup:** Total of 140+ linter warnings and technical debt cleared.

## Plan
### Completed
- [x] SQLite Transition (Phase 1A).
- [x] Manifest System (Phase 1B).
- [x] Embedded Workers (Phase 2).
- [x] Event-driven Sentinel (Phase 3A).
- [x] Security + 22 Unit Tests (Phase 4).
- [x] v1.1.6 Visual Grid (Enhanced).
- [x] Refactor Scanner — health integration.
- [x] v1.2.0 Core Toolkit — 7 modules, 32 CLI, 23 MCP tools.
- [x] project_info + portfolio_status aggregate MCP tools.
- [x] TUI — project detail enriched, health view git actions.
- [x] E2E test — 66/66 unit tests, CLI smoke, 23 MCP tools.
- [x] Phase 1 Refactor: `events.rs` monolith modularization.
- [x] Phase 2 Refactor: `dashboard.rs` UI panel modularization.

### In Progress
- [ ] 83-field AppState refactor (Phase 3B — Sub-states).
- [ ] Pull portfolio_status status column correctly from DB (memory.md content getting mixed up in some projects).

### Next Steps
- [ ] CI/CD status tracking (GitHub Actions API).
- [ ] build/test/deps columns in health view.
- [ ] Dependency graph in project detail view.

## Decision Log

| Date | Agent | Decision | Rationale |
|-------|-------|-------|-------|
| 2026-05-08 | Claude | SQLite Persistence | To prevent race conditions in JSON file writing and for O(1) query performance. |
| 2026-05-08 | Claude | Embedded Workers | For the application to run standalone without a daemon. |
| 2026-05-08 | Gemini | Non-blocking Render | To prevent UI thread from freezing while waiting for disk I/O. |
| 2026-05-08 | Claude | Event-driven Sentinel | To reduce CPU load and for instant response. |
| 2026-05-14 | Claude | AI-free Core Toolkit | Raios should not depend on AI API; every feature should be accessible as CLI + MCP. |
| 2026-05-14 | Claude | project_info aggregate tool | For agents to access all project info in 1 call instead of 5-8 (~5x token savings). |
| 2026-05-14 | Claude | 66 unit test baseline | Each module should be isolated for testing; CI preparation for regression detection. |
| 2026-05-15 | Claude | OnceLock regex cache | Regex'leri hot-path'te her seferinde compile etmemek için OnceLock ile tek seferlik derleme. |
| 2026-05-15 | Claude | Hybrid Memory = Cortex filter | Ayrı indeks yerine mevcut Cortex'e search_with_filter() eklendi — tek indeks, minimal diff. |
| 2026-05-15 | Claude | Instinct dual storage | Global JSON (hızlı erişim) + memory.md ## Instincts (okunabilirlik) — her ikisine de yaz. |

<!-- MCP update by antigravity at 2026-05-14 15:55 -->
- [2026-05-14 15:55] **Refactoring & Modularization Phase 1 Completed**: Successfully refactored the monolithic `src/app/events.rs` (1700+ lines) into a modular directory structure under `src/app/events/`.
- Created specialized sub-modules: `actions.rs`, `bg_messages.rs`, `commands.rs`, `keyboard.rs`, and `helpers.rs`.
- Resolved all visibility and import errors related to the split.
- Executed `cargo clippy --fix` resolving 116+ lint issues and technical debt.
- Verified compilation and binary build (`cargo build --bin raios`).
- Cleaned up scratch scripts and redundant code.
- Phase 1 of the refactor report is complete. Structure is now SRP-compliant.

<!-- MCP update by antigravity at 2026-05-14 16:07 -->
- [2026-05-14 16:07] **UI Panel Modularization Phase 2 Completed**: Successfully modularized the UI layer by splitting `src/ui/dashboard.rs` into specialized component files under `src/ui/panels/`.
- Created panels: `header`, `menu`, `content`, `recent`, `tasks`, `stats`, `rules`, `agents`, `policies`, `timeline`, `logs`, `help`, and `git_diff`.
- Updated `src/ui/mod.rs` to re-export all panels via the new `panels` module.
- Resolved unused imports and technical debt via `cargo clippy --fix`.
- Verified system stability with `cargo check`.
- Deleted the monolithic `dashboard.rs`.
- UI architecture is now component-based, significantly improving maintainability.

<!-- MCP update by antigravity at 2026-05-14 16:46 -->
- [2026-05-14 16:46] **R-AI-OS State Architecture Modularization Completed**: System-wide state architecture refactor successfully completed. Application state is now compartmentalized under the 'App' struct in namespaces (ui, system, projects, health, inventory, tasks, editor).

Actions:
1. Significant structural changes made to `src/app/state.rs`. Global fields moved to logical sub-structs.
2. `src/app/mod.rs`, `src/app/events/keyboard.rs`, `src/app/events/commands.rs`, `src/app/events/bg_messages.rs`, and `src/app/events/actions.rs` fully updated according to the new architecture.
3. All UI panels (`src/ui/panels/*.rs`) and main UI components (`projects.rs`, `search.rs`) transitioned to namespaced access.
4. All type mismatches and path errors resolved with `cargo check`.
5. `Debug` traits added to `Editor` and `RuleCategory` structs, `Default` implemented for `Editor`.

Result: A cleaner, modular, and more manageable codebase. All features (Project Detail, Health Check, Git Diff Approval, Task Dispatch) are running smoothly on the new structure.

<!-- MCP update by antigravity at 2026-05-14 16:50 -->
- [2026-05-14 16:50] **Final Validation & Documentation Update Completed**: All tests (66/66) passed successfully. README.md updated with new modular architecture details. Full stability achieved post-state refactor. Changes ready for Git push.
## Instincts
- OnceLock ile regex'leri bir kez compile et — scan_file hot-path'te her event'te yeniden derleme yapma
- search_with_filter'da önce tüm filtered sonuçları topla, sort et, sonra truncate et — take(top_k) sort'tan önce kullanma
- Yeni CLI komutları eklerken JSON serialization'da unwrap() kullanma — match + eprintln kullan
- GateGuard hook her Bash/Write/Edit öncesi facts gerektiriyor — her tool call öncesi 4 fact sun
