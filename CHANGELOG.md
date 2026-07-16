# Changelog

## v3.6.0 — 2026-07-17
### Added
- **Typed TUI control plane:** introduced the serialization-only `raios-contracts` crate, coherent daemon snapshots, typed commands/events, idempotency caching, transactional audit logging, and four attention-first routes: Now, Work, Explore, and Govern.
- **Windows installation:** added `install-system.ps1` for locked release builds, `%APPDATA%\raios` configuration/policy setup, user `PATH` registration, and the `RAIOS_Daemon` Scheduled Task.
- **Windows runtime parity:** added native Scheduled Task daemon management, `netstat.exe` PID lookup, PowerShell lifecycle hooks, PowerShell agent wrappers, and portable tray startup paths.
- **Cross-platform CI:** Windows, macOS, and Linux now build and test the full Cargo workspace with the lockfile enforced.
### Fixed
- Removed clippy blockers in control-plane, search, and preflight test paths; full workspace clippy now passes with `-D warnings`.
- Corrected Windows installation documentation, config/token paths, and stale README version/agent references.
### Verification
- `cargo test --workspace`: **678 passed, 2 ignored, 0 failed**.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `pnpm audit --audit-level=high`: no known vulnerabilities.

## v3.5.0 — 2026-07-11
### Changed
- **`raios grep` renamed to `raios locate`** (MCP `grep_search` → `locate_search`), including the core engine (`trigram::grep` → `trigram::locate`, `GrepMatch` → `LocateMatch`) — the command name no longer collides with "grep" as a concept.
### Fixed
- **MCP `semantic_search` now delegates to the resident Cortex daemon**, same as the CLI's `raios search` since v3.4.0. It previously paid a full in-process `Cortex::init()` + `index_project()` + HNSW rebuild on *every single call* — measured >60s per call, never completing within a 60s timeout. Now: ~1s warm, ~30-60s one-time cost only on the first call after an `aiosd` restart.
- **Duplicate search results from stale git worktrees.** `SKIP_DIRS` never listed `worktrees`, so any repo carrying a Claude Code isolated-worktree checkout under `.claude/worktrees/<id>/` reported every match twice (once from the real source, once from the stale copy) — confirmed on GT-Launcher (56MB stale checkout).
- **`tool_pin` (MCP tool-manifest tamper detection) was blocking all `tools/call` requests** after `grep_search`/`semantic_search` were added to the tool list without re-pinning. Verified the drift was legitimate (this repo's own commits), re-pinned.
### Added
- **Dart/Flutter support**: `dart` added to `INDEXED_EXTS`, verified on two real Flutter projects (NEXUS, esp32flutter) with exact-parity results via both CLI and MCP.
- `.fastembed_cache/` added to `SKIP_DIRS` (was being walked as source content).
- `raios-policy.toml` explicitly allow-lists `locate_search`/`semantic_search` and loopback (`127.0.0.1`/`localhost`) domains — the latter needed for the resident-daemon TCP client introduced in v3.4.0, consistent with this codebase's existing SSRF-hardening design (loopback was already deliberately excluded from the metadata-IP blocklist for this exact reason, see 2026-07-02 in memory.md).
### Notes
- Verified end-to-end by two agents independently: Antigravity Kaira implemented and live-tested the Dart support + MCP tool_pin fix; Claude Kaira independently re-verified that work (diff content, test counts) before merging, then found and fixed the `semantic_search` daemon-delegation gap and the worktrees duplicate-match bug through its own live dogfooding — both previously undocumented. Combined suite: **635 tests, 0 failures**.

## v3.4.0 — 2026-07-10
### Added
- **`raios grep <pattern>` — trigram-indexed exact/regex search** (`[--dir <path>] [-i] [--reindex]`). Every file's content is indexed as lowercased 3-character windows in the shared `workspace.db`; queries extract the literal substrings a pattern requires, intersect candidate files in SQL, then regex-verify only those candidates. Exhaustive within scope (grep semantics, not top-k), **measured at 0.015s warm** on this repo, with proven set-identical output to `grep -rn` over the same scope. Patterns yielding no usable ≥3-char literal (alternations, short wildcards) fall back to a full scoped scan — always correct, just slower. New MCP tool **`grep_search`** exposes the same engine to agents (200-match response cap).
- **Resident Cortex in `aiosd` — semantic search is now sub-second.** A dedicated worker thread owns ONE long-lived Cortex (embedding model + HNSW), serving requests over an mpsc/oneshot channel with lazy dirty-flag rebuilds (file-change events mark dirty; the next search rebuilds once). `raios search` transparently delegates its vector half to the daemon (300ms connect timeout, `AUTH` handshake) and **silently falls back to the full in-process path when the daemon is unreachable** — measured: ~1.0s daemon-warm vs ~4-6s fallback, identical results. New daemon command `CortexReindex`; `VectorSearch` responses gain an additive `vector_hits` field (existing `results` shape untouched for TUI compatibility).
### Fixed
- Daemon's `VectorSearch` handler previously ran `Cortex::init()` — full model load + HNSW rebuild — **on every single request**; the file-watcher worker did the same per changed file, and its incremental `index_file` writes never triggered an HNSW rebuild at all (silently useless indexing). All three replaced by the resident-worker design above.
### Notes
- Built as two parallel agent worktrees (Codex: trigram; Antigravity: daemon residency) with an explicitly partitioned file surface; merged sequentially with a rebase. Combined suite: **634 tests, 0 failures** (619 → +10 trigram, +5 daemon).

## v3.3.0 — 2026-07-09
### Added
- **Layered memory (L0→L3), ported from TencentDB-Agent-Memory's semantic pyramid**: `mem_nodes` (immutable evidence: raw transcript lines, archived body revisions) + `mem_lineage` (derived-from/revision edges) give `mem_items` real traceability for the first time. `mem_items.layer` discriminates L1 atomic facts (deterministic hash-slugged, deduped), L2 daily scene digests (cumulative same-day merge with `[[slug]]` backlinks), and L3 a rolling persona (background + working rules, rebuilt from the newest L1 facts). All distillation is local/deterministic — no LLM calls.
- **`raios mem history <slug>`** and **`raios mem list --layer <n>`** — inspect a memory item's revision chain and filter by pyramid layer.
- **`raios sessions --canvas <session_id>`** — folds a session's `session_events` stream into a compact Mermaid flowchart; consecutive same-type events collapse into one node with a `se:<id>` back-reference to the full, untruncated payload — compression is never irreversible.
- **`raios usage` now reports live Claude Pro/Max quota remaining.** The statusLine script caches `rate_limits.five_hour`/`seven_day` usage percentages (from Claude Code's own stdin JSON) to `~/.claude/raios-usage-cache.json`; `raios usage` reads that cache (with a 24h staleness cutoff) and shows `5h:XX% 7d:YY% remaining` plus formatted reset times instead of a hardcoded "unknown".
### Fixed
- **`mem_items.body` unbounded growth**: `mem_upsert` previously concatenated every write onto the same row forever. It now replaces the body and archives the previous version as an immutable `mem_nodes` revision — the full history is still recoverable via `raios mem history`, but the live row stays bounded.
- **A second instance of the same bug, caught only by a whole-branch review**: the 90-second periodic memory-sync thread re-scans the entire session transcript on every tick (fixed start timestamp), and was inserting a fresh, undeduplicated `mem_nodes`/`mem_lineage` row per matched fact on every pass — silently recreating unbounded growth one layer down. Fixed with content-addressed dedup on `(project_key, kind, content)` for `l0_raw` nodes (revision nodes are correctly exempt — each is a genuinely distinct snapshot).
- `mem_upsert`'s archive-then-replace sequence (revision node + lineage edge + item update) now runs inside a single SQLite transaction instead of three unguarded autocommit statements.
- `raios security` / `raios refactor` output now discloses its own limitation inline ("pattern-based scan — a clean result is not proof of absence") instead of only in internal docs — both are regex/heuristic scanners, not semantic analysis.
- `gen-context.config.json` used an unrecognized `customOutput` key that `sigmap` silently ignored, so it never wrote `SIGMAP.md` (it was defaulting to `.github/copilot-instructions.md` instead). Corrected to the real `output` key.
### Changed
- `session_memory.rs` (974 lines after the memory-layering work) split into a `session_memory/` directory module — `transcript_io.rs`, `heuristics.rs`, `distillation.rs`, plus a thin `mod.rs` orchestrator. Pure move, no behavior change; full external call surface (`auto_sync_agent_memory`, `collect_transcript`, `decision_lines_from_transcript`, etc.) preserved.

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
