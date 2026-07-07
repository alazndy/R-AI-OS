# Changelog

## v3.2.0 — 2026-07-07
### Changed
- **Workspace split**: monolithic library physically split into a Cargo workspace of 5 crates (`raios-core`, `raios-runtime`, `raios-surface-cli`, `raios-surface-mcp`, `raios-surface-tui`) — the actual reason for the version jump from v3.0.0; no v3.1.0 was ever tagged.
- **Schema consolidation**: `task_graph_nodes`/`swarm_tasks` control-plane link columns moved into the single central migration (`db/schema.rs`); both stores previously carried their own duplicate `CREATE TABLE`/`ALTER TABLE` that had silently drifted out of sync.
### Fixed — security hardening pass (2026-07-07 session)
- `.ipc_token` (legacy daemon auth file) removed entirely — it duplicated `.session_token`'s secret without matching its owner-only (0600) permissions, defeating that hardening for any local reader. All clients now read `.session_token` only.
- Daemon AUTH handshake now uses a constant-time comparison (was a plain `!=`).
- Session tokens and the hub API key now come from a direct OS CSPRNG draw instead of a hand-rolled `SHA256(uuid‖pid‖time)` construction.
- `[server.hub] trusted_proxy` config added — a same-host reverse proxy no longer silently downgrades remote requests onto the localhost auth path.
- Windows: token files now get an owner-only ACL on write (`icacls`) **and** are verified on read, not just written — previously Windows had no permission enforcement at all.
- `raios hub api-key show` masks the key by default now (`--reveal` for the full value) instead of always printing it in full.
### Note
This entry summarizes the ~90 commits between v3.0.0 and this tag rather than itemizing each — see git log for full detail. Full findings/fix trail for the security pass: `docs/adversarial-review-2026-07-07.md`.

## v3.0.0 — 2026-06-25
### Added
- **4-agent identity matrix**: Claude Kaira, Codex Kaira, OpenCode Kaira, Antigravity Kaira — Gemini CLI fully retired (Google shut it down).
- **`raios handoff`** — atomic agent-to-agent handoff built entirely on the existing control plane (`cp_tasks` / `cp_agent_runs` / `cp_artifacts` / `cp_approvals`), no separate state file. `--to`/`--status` are clap-validated; `--msg` is heuristically scanned for secrets (AWS/Anthropic/OpenAI/GitHub keys, PEM blocks) and refused before it ever touches the DB. Auto-attaches `git diff --stat HEAD` so the receiving agent sees what changed without being told. A new handoff to the same `(agent, project)` atomically supersedes any still-pending one instead of letting stale handoffs pile up.
- **Real prompt delivery, not a dead env var** — `raios run`/`raios task` deliver the pending `[HANDOVER CONTEXT]` via each CLI's actual prompt-injection surface: `claude --append-system-prompt`, `codex <positional prompt>`, `opencode --prompt`, `agy --prompt-interactive`. Delivered exactly once (consumed only after the child process actually spawns).
- **`agy` (Antigravity CLI) support** — `raios run agy`/`raios run antigravity` now spawn the real `agy` binary instead of erroring "Unsupported agent".
- **TUI Inbox panel** — new 14th menu screen showing pending approvals (handoffs included), active agent runs, and blocked tasks straight from the control plane; previously this data was only reachable via the `get_inbox` MCP tool, invisible at the terminal.
### Fixed
- `create_handoff_workflow` now runs inside a real SQLite transaction (`unchecked_transaction`) instead of unguarded sequential inserts.
- `run_agent` only marks a handoff "consumed" after `cmd.spawn()` actually succeeds, not before.
- `cp_list_personal_tasks` was leaking workflow tasks (file-change-approval, handoff) into the personal-task checklist sidebar; now excludes any task with an attached `cp_approvals` row.
- Superseding a stale pending handoff now also cancels its `cp_agent_runs` row — previously it lingered as `awaiting_approval` forever, cluttering the Inbox panel.
- A stale, never-committed `lock_manager.rs` test left broken by the gemini→claude rename was asserting against itself.

## v1.5.1 — 2026-05-21
- (no commits since last tag)

All notable changes to the **R-AI-OS** project will be documented in this file.

## [1.5.0] - 2026-05-21 (Intelligence & Architecture Edition)

### Added

**Phase 5 — Agent Swarm Mesh:**
- `SwarmStore` — SQLite-backed worktree lifecycle management.
- 5 TCP commands: `CreateSwarmTask`, `GetSwarmTask`, `ListSwarmTasks`, `ApproveSwarmTask`, `RejectSwarmTask`.
- `raios swarm start|list|approve|reject` CLI subcommand.
- 3 MCP tools: `create_swarm_task`, `list_swarm_tasks`, `approve_swarm_task`.

**Phase 6 — Edge Intelligence:**
- `EdgeRouter` — cosine-similarity semantic routing of natural-language queries to capability names.
- `raios route "<query>"` CLI command.
- `route_capability` MCP tool.

**Phase 7 — Evolutionary Intelligence:**
- `CandidateStore` — learns instinct candidates from job success/failure outcomes.
- `start_evolution_worker` subscribes to daemon broadcast and auto-generates instinct candidates.
- TCP commands: `ListEvolutionCandidates`, `PromoteEvolutionCandidate`, `PruneExpiredCandidates`.
- `raios evolve list|promote|prune` CLI subcommand.
- MCP tools: `list_evolution_candidates`, `promote_evolution_candidate`.

**Phase 8 — Recursive Reasoning (Task DAG):**
- `TaskGraph` module — directed acyclic graph (DAG) of dependent shell commands.
- SQLite persistence with cycle detection, depth validation (max 50 nodes), 10-min timeout.
- TCP commands: `CreateTaskGraph`, `ExecuteTaskGraph`, `GetTaskGraph`.
- `execute_graph_async` — parallel execution of independent nodes via Factory Mode.
- 5 unit tests: create, ready_nodes, max_limit, mark_complete, cycle detection.

### Refactored

**Codebase Architecture (3-phase refactor):**
- `src/cli.rs` (3001 lines) split into `src/cli/` — 11 submodules, max 329 lines each (`dev`, `git`, `health`, `instinct`, `new`, `search`, `security`, `swarm`, `version`, `workspace`).
- `src/mcp_server.rs` (1667 lines) split into `src/mcp/` — 7 submodules: `mod`, `resources`, `tools`, `tools_workspace`, `tools_dev`, `tools_git`, `tools_swarm`.
- `hybrid_search.rs` + `indexer.rs` → `src/search/` module with backwards-compat re-exports.
- `edge.rs` + `evolution.rs` + `instinct.rs` + `router.rs` → `src/intelligence/` module.

**Code Quality:**
- 33 clippy warnings → 0 (`sort_by_key`, `&Path`, `strip_prefix`, `while_let`, dead fields, `flatten`, identical blocks, manual division).
- Risky `unwrap()` calls replaced in production paths (`daemon/server.rs`, `cli/search.rs`, `cli/health.rs`, `cli/new.rs`).

### Changed
- `lib.rs`: 42 top-level modules → logical groupings with backwards-compatible `pub use` aliases.
- All previously scattered `crate::hybrid_search`, `crate::indexer`, `crate::edge`, `crate::evolution`, `crate::instinct`, `crate::router`, `crate::mcp_server` paths continue to work unchanged.

## [1.4.0] - 2026-05-20 (Universal Kernel Edition)

### Added

**Universal Agent Kernel (R-AI-OS 2.0):**
- **Tri-Protocol Interface:** Daemon TCP (:42069), MCP-over-TCP (:42070) ve CLI aynı anda çalışıyor. Claude, Gemini, Codex ve Antigravity aynı event bus'ı paylaşıyor.
- **Lock Manager:** Dosya ve task bazlı kilit yöneticisi — öncelik sistemi (User > Agent > Automation), 30s timeout, re-entrant kilitleme, deadlock önleme. 5 unit test.
- **Radar Whisper Stream:** Compile hataları, security açıkları ve mimari ihlaller bağlı tüm ajanlara gerçek zamanlı `RadarWhisper` eventi olarak push ediliyor.
- **Factory Mode:** Ağır görevler (refactor, test generation, build) arka planda kuyruklanıyor. Anında `job_id` dönüyor, tamamlanınca SQLite inbox + webhook notification. 4 unit test.
- **Universal Proxy-Store:** Yeteneği isteyen ajan ile arka taraftaki backend'i (Rust internal / Python skill / Shell / MCP bridge) soyutlayan proxy katmanı. 5 unit test.

**Storage Overhaul:**
- **Cortex SQLite Store:** Vector embeddings `cortex_store.json`'dan SQLite BLOB'a taşındı. 384×f32 = 1536 byte/chunk little-endian BLOB. Upsert artık transaction-safe (split-brain fix). Legacy JSON otomatik siliniyor.
- **BM25 Index Persistence:** Inverted index SQLite'a persist ediliyor. Restart'ta dosya mtime'larına göre sadece değişen dosyalar yeniden indexleniyor — büyük workspace'lerde soğuk başlangıç eliminasyonu.
- **Session Memory System:** Her agent TCP bağlantısı otomatik session açıyor. Olaylar (`file_change_request`, `handover`, `note`) SQLite'a yazılıyor. Disconnect'te `memory.md`'ye özet satırı ekleniyor.

**MCP Enhancements:**
- `raios://session/current` — aktif session'ı events ile birlikte döndüren MCP resource.
- `raios://session/recent` — son 10 tamamlanmış session.
- `session_note` tool — agent'ın bir kararı veya tamamlanan görevi session memory'ye kaydetmesi için.

### Fixed
- `instinct.rs`: `ProjectHealth` struct'ında eksik `ci_status` / `ci_url` alanları düzeltildi.

### Changed
- `aiosd` daemon artık `Kernel::run()` üzerinden başlıyor — tüm protokoller paylaşılan broadcast channel üzerinde.
- `Server::run()` → `run_inner(tx)` refactoru ile dışarıdan broadcast kanalı alabilir hale getirildi.

---

## [1.3.0] - 2026-05-17 (AI Intelligence Layer)

### Added
- **Official Documentation Suite:** Kapsamlı 7 bölümlük Wiki ve "AI OS Kernel" odaklı yeni README.md.
- **Hybrid Memory Search:** `raios memory --query` ile tüm projelerde semantic + BM25 arama desteği.
- **Sentinel Guard Watch:** `raios security --watch` ile gerçek zamanlı OWASP güvenlik taraması ve Windows bildirimleri.
- **Instinct Automation:** `raios instinct suggest` ile projelere otomatik kodlama kuralları önerisi.
- **CI Status Tracking:** `raios ci` ile GitHub Actions iş akışlarının TUI üzerinden canlı izlenmesi.
- **Aura Hardened IPC:** UUID tabanlı güvenli daemon/client haberleşme protokolü.
- **Diff Inbox Pattern:** Kod değişiklikleri için asenkron onay kuyruğu.

### Fixed
- Cortex Engine'de bellek kullanımı ve indeksleme performansı iyileştirildi.
- TUI üzerindeki gecikme (lag) sorunları SQLite önbelleği ile giderildi.
- Daemon bağlantı kopma ve sonsuz döngü hataları düzeltildi.

## [1.2.0] - 2026-05-14 (AI OS Kernel)

### Added
- **Core Toolkit:** Git, Build, Deps, Env, Version, Process ve Disk yönetimi için 7 yeni modül.
- **AgentShield Guard:** Tehlikeli komut engelleme ve veri sızıntısı koruması.
- **Aggregate MCP Tools:** `project_info` ve `portfolio_status` ile ajanlar için optimize edilmiş veri akışı.
- **Modular App State:** Uygulama durumu namespaced struct'lara (ui, system, projects vb.) bölündü.
- **TUI Enhancements:** Zenginleştirilmiş proje detay paneli ve sağlık raporları.

## [1.1.0] - 2026-05-10

### Added
- **Zero-Setup Wizard:** 8 adımlı otomatik kurulum sihirbazı.
- **OWASP Security Scanner:** Proje bazlı güvenlik skorlaması.
- **Mempalace Scanner:** Derinlemesine proje envanter taraması.

## [1.0.0] - 2026-05-01 (Genesis)

### Added
- **Full CLI Power Tools:** Temel R-AI-OS komut setinin tamamlanması.
- **TUI Improvements:** Stabil ve hızlı Terminal UI arayüzü.

## [0.9.0] - 2026-04-25

### Added
- **GitHub Remote Support:** Uzak depo URL desteği ve workspace optimizasyonu.

## [0.8.0] - 2026-04-15 (Foundations)

### Added
- **Modular TUI:** Bileşen tabanlı arayüz mimarisi.
- **aiosd Daemon:** Arka plan servis altyapısı.
- **Agent Security Proxy:** Ajan etkileşimleri için güvenlik katmanı.

---
*Generated by R-AI-OS Automator*
