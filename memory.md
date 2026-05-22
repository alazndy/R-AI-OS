# R-AI-OS Memory

## Current Status
- Date: 2026-05-22
- Active agent: Gemini (v1.5.1 — production)
- Version: v1.5.1
- Version Name: Android Intelligence Edition
- Status: **Production-ready.** Android/Gradle support fully integrated. Full test suite (170/170) green. 0 clippy warnings.

## Gemini
### Achievements
- **Android/Gradle Support (v1.5.1):** Full lifecycle support for Android projects:
  - **Detection:** `ProjectType::Android` logic in `build.rs` and `deps.rs`.
  - **Build:** `gradlew assembleDebug/Release/compileDebugKotlin` with `--release` and `--check` flags.
  - **Test:** Unit (`testDebugUnitTest`) and Instrumented (`connectedDebugAndroidTest`) runners.
  - **Deps:** `libs.versions.toml` (Version Catalog) parsing and outdated detection.
  - **Version:** Automatic `versionName` and `versionCode` read/write from `app/build.gradle`.
- **Clippy Cleanup:** Resolved technical debt including `large_enum_variant` in `BgMsg` and `new_without_default` in `AgentRouter`.
- **Final Validation:** Verified all features on `GT Launcher` (Android) and pushed to origin.
- **SIGMAP Tracking:** `has_sigmap` columns added to R-AI-OS Health Dashboard.
- **Project Documentation:** Official Wiki and README.md rewritten for "Aura Hardened Edition".

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

## Feature Inventory (v1.5.1 — 2026-05-21)

### CLI Commands (35 total)
| Command | Description |
|---------|-------------|
| `raios health` | Portfolio health dashboard |
| `raios health --json` | JSON output |
| `raios security [--watch] [--json]` | OWASP scan or file-watch mode |
| `raios memory --query <text> [--top N]` | Hybrid semantic search across all project memory files |
| `raios instinct add/list/suggest` | Instinct management (global ~/.agents/instincts.json + per-project memory.md) |
| `raios task list/add/done/dispatch` | Task management with agent dispatch |
| `raios git status/log/diff/commit/push/pull/branch/checkout` | Git operations (9 commands) |
| `raios build [--test]` | Project build + test runner (Rust/Node/Python/Go) |
| `raios deps [--audit]` | Outdated deps + CVE scan |
| `raios env` | .env vs .env.example diff, missing/empty key detection |
| `raios version bump/changelog/tag` | Semver bump, CHANGELOG.md, git tag |
| `raios process ports/list/kill-port` | Port list, process list, kill |
| `raios disk size/clean` | Project size analysis, cache cleaning |
| `raios swarm add/list/merge/status` | Agent Swarm Mesh (5 TCP nodes) |
| `raios route` | EdgeRouter — model/agent routing |
| `raios evolve` | CandidateStore — evolutionary intelligence |
| `raios graph` | TaskGraph DAG (recursive reasoning) |
| `raios workspace sync/status` | Workspace sync |
| `raios ci status` | GitHub Actions CI/CD status |

### MCP Tools (23 total)
- `project_info` — aggregated git+health+version+env+disk in 1 call (~5x token savings)
- `portfolio_status` — summary table for all 42 projects
- Git: status, log, commit, push (4)
- Build: build, test (2)
- Deps: check (1)
- Env: check (1)
- Version: bump, release (2)
- Process: port_list (1)
- Disk: analyze (1)
- Swarm: add_node, list_nodes, merge (3)
- Route: route (1)
- Evolve: submit, list (2)
- Graph: ask_architect (1)

### TUI Views
| View | Access | Key Features |
|------|--------|-------------|
| Dashboard | boot | Task panel, menu navigation, agent launcher |
| Health View | `h` | BUILD/TEST/DEPS/COMPLIANCE/SECURITY/RFCT/MEM/SIG columns; `[b]` triggers build/test/deps on selected project; `[c]` commit, `[p]` push |
| Project Detail | `Enter` on project | memory.md viewer, git log, graphify, git diff |
| MemPalace | `m` | Room-based project navigation with filter |
| Sentinel | `s` | File watch with OWASP alerts |
| Search | `Ctrl+P` | Hybrid semantic search |
| Git Diff | `i` | File change approval UI (`y`/`n`) |
| Graph Report | `r` in detail | Graphify output viewer |
| Setup Wizard | first boot | 7-step workspace configuration |

### Module Structure (v1.5.1)
```
src/
├── cli/          (11 modules) — all CLI commands
├── mcp/          (7 modules)  — MCP server + tools
├── app/
│   └── events/
│       ├── keyboard/  (7 modules: mod, dashboard, editor, graph, health, mempalace, project)
│       ├── actions.rs
│       ├── bg_messages.rs
│       ├── commands.rs
│       └── helpers.rs
├── security/     (3 modules: mod, patterns, scanner, audit)
├── intelligence/ — instinct, memory search
├── swarm/        — mesh, store, merge, worktree
├── core/         — build, deps, env, git, version, process, disk, ci
├── ui/
│   └── panels/   (13 components)
├── daemon/       — IPC, state, health, validation
└── cortex/       — BM25 hybrid search, chunker, embedder
```

### Key Invariants / Architecture Decisions
- Health check (`check_project`) is fast — build/test/deps NOT auto-run; triggered by `[b]` in TUI
- `project_info` MCP aggregates 5 tools in 1 call — always prefer over individual calls
- `OnceLock` for all regex compilation in security scanner — never recompile in hot path
- `check_project_with_security()` for security scan, `check_build_test_deps()` for build/test/deps
- SQLite health_cache for O(1) portfolio queries — never re-scan disk synchronously in UI render
- Swarm: 5 TCP nodes max, merge via git worktrees

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
- [x] Phase 3B Refactor: 83-field `AppState` compartmentalized into sub-states (ui, system, projects, health, etc.).
- [x] Database Data Integrity: Fixed `portfolio_status` column pulling `memory.md` contents by applying strict prefix/keyword matching in `extract_status`.
- [x] Security Model Documentation (`docs/WIKI/02-Security-Model.md`).
- [x] Phase 9: IDE Symbiosis (MVP + Full Integration) — VS Code extension, Daemon IPC bridge, Jump-to-Code.
- [x] CI/CD status tracking (GitHub Actions API) integrated into TUI.

### In Progress

### Next Steps
- [x] **build/test/deps columns in health view (TUI)** — ProjectHealth'e build_ok/test_passed/test_failed/deps_outdated/deps_cve eklendi. `[b]` ile selected project için background trigger. ✅ 2026-05-21
- [x] **Faz B: keyboard.rs (1032→7 modül) + security.rs (907→4 modül) bölünmesi.** ✅ 2026-05-21
- [x] **v1.5.0 Release** — Intelligence & Architecture Edition. ✅ 2026-05-21
- [x] **Codebase Refactor** — cli/ (11 modules), mcp/ (7 modules), search/, intelligence/. 0 clippy warnings. ✅ 2026-05-21
- [x] **Phase 8: Recursive Reasoning** — TaskGraph DAG. ✅ 2026-05-21
- [x] **Phase 5: Agent Swarm Mesh** — SwarmStore, 5 TCP, CLI, 3 MCP tools. ✅ 2026-05-21
- [x] **Phase 6: Edge Intelligence** — EdgeRouter, route CLI, MCP tool. ✅ 2026-05-21
- [x] **Phase 7: Evolutionary Intelligence** — CandidateStore, evolve CLI, 2 MCP tools. ✅ 2026-05-21

## Research References
- **Agent Discipline:** [addyosmani/agent-skills](https://github.com/addyosmani/agent-skills)
- **Local Routing:** [cactus-compute/needle](https://github.com/cactus-compute/needle)
- **Skill Evolution:** [HKUDS/OpenSpace](https://github.com/HKUDS/OpenSpace)
- **Deep Logic:** [alexzhang13/rlm](https://github.com/alexzhang13/rlm)
- **Workspace Mgmt:** [horang-labs/tessera](https://github.com/horang-labs/tessera)

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

<!-- MCP update by antigravity at 2026-05-17 16:27 -->
- [2026-05-17 16:27] **Enflow Orchestrator Script Integration**: - Created run.js, run.sh, and run.bat to orchestrate both the frontend and backend.
- Added auto-restart mechanism that automatically kills processes on ports 3000 and 3002.
- Colored process logging (green/cyan prefixing) for clean console observation.
- Added dev/start scripts to backend package.json.
- Updated memory.md and pushed all changes safely to git origin.
