# R-AI-OS Memory

## Current Status
- Date: 2026-06-04
- Active agent: Antigravity
- Version: v2.0.0-alpha
- Version Name: Aura Hardened Kernel
- Status: **Alpha Phase.** Security Kernel (Faz 1-4) fully implemented and tested (239/239 green). TUI Security panel live.

## Gemini
### Achievements
- **Hardened Kernel Foundation (v2.0.0-alpha):** Transitioned Raios into a "Universal Agent Kernel" with a focus on security and observability.
- **Vigils Integration (Security):**
  - **Hash-Chained Audit Ledger:** Implemented a tamper-evident event logging system. Each entry is linked via SHA-256 hashes stored in SQLite, inspired by the Vigils control plane.
  - **Redaction Engine:** Developed a regex-based masking system to automatically detect and hide sensitive data (OpenAI keys, GCP secrets, PII) from logs and external monitoring.
- **Sentry Observability:** Integrated Sentry SDK for real-time error tracking and panic handling with breadcrumbs.
- **Modular Kernel Architecture:** Established `src/kernel/` with `security/` and `observability/` sub-packages.
- **Dependency Update:** Added `sentry`, `sha2`, and `regex` to `Cargo.toml`.

## Antigravity
### Achievements
- **Table-Based Health UI:** Dashboard list refreshed with `ratatui::Table`.
- **Binary Recovery:** File locking issues on Windows resolved via process management.
- **Refactor & Modularization:**
  - `src/app/events.rs` (1700+ lines) → `src/app/events/` module (actions, bg_messages, commands, keyboard, helpers).
  - `src/ui/dashboard.rs` (900+ lines) → 13 separate modules under `src/ui/panels/`.
  - `src/core/build.rs` (1352 lines) → `src/core/build/` (10 sub-modules, <300 lines each).
  - `src/core/deps.rs` (670 lines) → `src/core/deps/` (10 sub-modules).
  - `src/app/events/keyboard.rs` (1032 lines) → `src/app/events/keyboard/` (6 sub-modules).
- **VSCode Extension Fix:** `raios-0.3.3.vsix` — resolved `ENOENT` with `resolveRaiosBinary`.
- **Global `--refactor` CLI flag**, **E2E validation suite**, **Clippy cleanup (140+ warnings)**.
- **Security Research:** [vigils_analysis_report.md](file:///C:/Users/turha/.gemini/antigravity-cli/brain/a16d4419-6df1-40b5-84c9-19e5ea53ce31/vigils_analysis_report.md) — Filesystem Jail, Egress Filter, Policy Manager, Hash-Chain proposals.

### Security Kernel (2026-06-04) — ALL 4 PHASES COMPLETE ✅
| Faz | Modül | Entegrasyon | Test |
|-----|-------|-------------|------|
| 1 – Filesystem Jail | `src/security/sandbox.rs` | `resolve_git_path()` in `mcp_server.rs` | 6 test ✅ |
| 2 – Policy Manager | `src/security/policy.rs` + `raios-policy.toml` | `handle_tools_call()` gate | 4 test ✅ |
| 3 – Verify Chain | `src/security/verify_chain.rs` + `audit_log` table | Policy gate logs every allow/deny | 4 test ✅ |
| 4 – Egress Filter | `src/security/egress.rs` | `raios-policy.toml [egress]` section | 7 test ✅ |

### Refactor & Code Health (2026-06-04) ✅
- **False-Positive Risk Pattern Eliminasyonu:** `src/refactor_scan.rs` dosyası güncellenerek `.unwrap()` ve `.expect()` aramalarının test modülleri (`mod tests`), test anotasyonları (`#[test]`) ve assertion (`assert!`) satırlarında false-positive üretmesi engellendi.
- **CLI Workspace Refactor:** `src/cli/workspace.rs` içindeki tüm JSON serileştirme çıktılarındaki `.unwrap()` çağrıları `.unwrap_or_default()` ile değiştirilerek kod daha güvenli hale getirildi ve listeden tamamen düşürüldü.

### Hybrid UI Development (2026-06-04) — FAZ 2A & 2C COMPLETE ✅
- **Webview Sidebar Panel (Faz 2A):** VS Code sidebar'ına `raios-sidebar-view` (Activity Bar icon) ve `raios.sidebar` Webview eklendi. Geist Sans ve glassmorphism temalı read-only Kanban dashboard'u entegre edildi.
- **IPC Token Bridge (Faz 2C):** Oturum token'ının sızmasını önlemek amacıyla TokenBridge proxy eklendi. Extension host, talepleri daemon'a (port 42071) Bearer authorization token ile güvenli bir şekilde proxy ediyor.
- **VSix Packaging (Faz 3):** Extension compile ve vsce packaging adımları test edildi. `raios-0.4.0.vsix` başarıyla oluşturuldu.

**Key design decisions (Claude feedback + Antigravity implementation):**
- Hash-chain is "tamper detection" not "tamper prevention" — correctly classified.
- Policy fails closed: `confirm` = deny in stdio/daemon mode (no interactive prompt available).
- Egress filter at MCP tool hook level (not OS network layer — scoped to what raios controls).
- `raios-policy.toml` is the single config source (separate from MASTER.md).
- Policy gate → audit ledger connected: every tool call is logged before execution.

### TUI Update (2026-06-04) ✅
- **`src/ui/panels/policies.rs`** rebuilt: reads live data from `raios-policy.toml` and SQLite.
  - Sandbox: ACTIVE/DISABLED + blocked path count
  - Tool Policy: default action color + deny/confirm rule list (up to 4)
  - Egress Filter: ACTIVE/DENY ALL/OFF + domain counts
  - Audit Chain: total event count + verify-chain hint

## Feature Inventory (v2.0.0-alpha — 2026-06-04)
| Feature | Description | Inspired By |
|---------|-------------|-------------|
| **Filesystem Jail** | Canonicalize + workspace boundary enforcement, SandboxGuard | Vigils |
| **Policy Manager** | TOML-based tool allow/deny/confirm engine | Vigils |
| **Audit Ledger** | Tamper-evident SHA-256 chained SQLite logs + verify-chain CLI | Vigils |
| **Egress Filter** | Domain allowlist/blocklist, suffix match, fail-closed | Vigils |
| **Redaction** | Automatic PII/Secret masking in all logs | Vigils |
| **Sentry Monitoring** | Error tracking with deep contextual breadcrumbs | Sentry |
| **Panic Hook** | Automatic crash reporting with full session context | Sentry |

### CLI Commands (36 total)
| Command | Description |
|---------|-------------|
| `raios verify-chain` | Verify audit log hash-chain integrity |
| `raios verify-chain -n N` | Show last N entries then verify |
| `raios security` | OWASP security scan |
| `raios health` | Portfolio health dashboard |
| `raios git status/log/diff/commit` | Git operations |
| `raios build/test/deps/env` | Dev tools |
| `raios swarm start/list/approve` | Parallel agent worktrees |
| ... (full list in `raios --help`) |

## Plan
### Completed
- [x] Phase 1-7: Core functionality, TUI, Swarm, and Intelligence features (v1.5.1).
- [x] **Phase 8: Refactor & Modularization.** ✅ 2026-06-04
- [x] **Phase 10: Hardened Kernel (Alpha).** Sentry & Vigils integration. ✅ 2026-06-03
- [x] **Phase 10B: Security Kernel (Faz 1-4).** Sandbox + Policy + Chain + Egress. ✅ 2026-06-04
- [x] **GitHub Araştırma Görevi:** vigils, delego, ruvos, bashagt, thread-sentry analizleri ve entegrasyon planı tamamlandı. ✅ 2026-06-04
- [x] **Hibrit UI Planlama Aşaması:** Mimari plan oluşturuldu (Tauri iptal edildi). ✅ 2026-06-04
- [x] **Hibrit UI Geliştirme (Faz 2A & 2C):** VS Code Sidebar WebView & TokenBridge proxy tamamlandı. ✅ 2026-06-04

### In Progress
- [ ] **Phase 11: Tool Pinning & Drift Detection** (MCP tool hash verification — detects supply chain tampering).
- [ ] **Phase 12: Secret Leasing** (Temporary env vars for agents, auto-revoke after session).

### Next Steps (Öncelik Sırası)
1. **Tool Pinning:** MCP tool manifest'ini hash'le, drift olursa uyar/reddet.
2. **Secret Leasing:** `raios secret grant <tool> <ENV_VAR>` → TTL-based env injection.
3. **Faz 2B (memory.md Write-Back):** Sonraki sprint kapsamında sidebar üzerinden memory.md task state güncelleme.
4. **Rate Limiting:** Tool call frequency limiter (spam protection for AI loops).
5. **Quarantine Mode:** Şüpheli ajan çağrısında sandbox'a al, human approval bekle.

## Research References
- **Ajan Kontrol Düzlemi:** [duncatzat/vigils](https://github.com/duncatzat/vigils)
- **Agentic OS:** [dgdev25/ruvos](https://github.com/dgdev25/ruvos)
- **Minimalist Agent:** [lloydzhou/bash-agent](https://github.com/lloydzhou/bash-agent)
- **Agent Discipline:** [addyosmani/agent-skills](https://github.com/addyosmani/agent-skills)
- **Local Routing:** [cactus-compute/needle](https://github.com/cactus-compute/needle)

## Decision Log

| Date | Agent | Decision | Rationale |
|-------|-------|----------|-----------|
| 2026-06-04 | Antigravity | Webview Sidebar & TokenBridge Entegrasyonu | Güvenlik sınırlarını korumak için token Webview'a verilmedi, Extension Host proxy kullanıldı. |
| 2026-06-04 | Antigravity | GitHub Repolarının Analizi ve Entegrasyon Kararları | Vigils/Delego (Kapsandı/Entegre), Ruvos (Hafıza Ref), Bashagt (Hafif Worker), Thread-Sentry (Dev-Dependency) olarak belirlendi. |
| 2026-06-04 | Antigravity | `raios-policy.toml` ayrı config | MASTER.md'den bağımsız, proje-spesifik güvenlik kuralları |
| 2026-06-04 | Antigravity | Confirm = fail-closed in stdio | Interactive prompt yok → güvenli default deny |
| 2026-06-04 | Antigravity | Egress at MCP hook, not OS level | Raios'un kontrol edebildiği katman; WFP overkill ve platform-specific |
| 2026-06-04 | Antigravity | Hash-chain = tamper detection, not prevention | SQLite değiştirilebilir; forensic tool olarak doğru sınıflandırma |
| 2026-06-04 | Antigravity | Policy gate → audit ledger bağlantısı | Her tool çağrısı loglanır → chain bütünlüğü anlamlı hale gelir |
| 2026-06-03 | Gemini | Hash-Chained Audit Ledger | Multi-agent aktivite tamper-evident olsun, merkezi kayıt |
| 2026-06-03 | Gemini | Redaction-Before-Log | PII ve secrets local log veya Sentry'ye sızmasın |
| 2026-06-03 | Gemini | Sentry for OS Kernel | Profesyonel observability, production panic debug |


