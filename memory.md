# R-AI-OS Memory

## Son Durum
- Tarih: 2026-05-14
- Aktif agent: Claude (v1.2.0 tamamlandı, E2E testler geçti)
- Sürüm: v1.2.0
- Sürüm Adı: Core Toolkit
- Durum: **Production-ready.** 7 core modül, 32 CLI komutu, 23 MCP tool, 66 unit test — hepsi yeşil. TUI zenginleştirildi, health view'dan git commit/push yapılabiliyor. `raios_project_info` aggregate MCP tool hazır.

## Claude
### Yaptıkları
- **Faz 1A: SQLite Migration:** `entities.json` yapısı tamamen `rusqlite` tabanlı SQLite veritabanına taşındı.
- **Faz 1B: Manifest System:** `.raios.yaml` manifest desteği eklendi.
- **Faz 2: Embedded Workers:** `workers.rs` modülü ile daemon olmadan standalone çalışma sağlandı.
- **Faz 3A: Event-driven Sentinel:** `notify` kütüphanesiyle olay güdümlü yapıya geçildi.
- **Faz 4: Security & Testing:** Semgrep + 22 unit test.
- **Refactor Scanner:** `src/refactor_scan.rs` — satır sayısı, unwrap zinciri, nesting derinliği tespiti. Health view'a REFACTOR kolonu, dashboard'a uyarı eklendi.
- **Core Toolkit (v1.2.0):** `src/core/` katmanı — 7 modül, tümü CLI + MCP tool olarak erişilebilir:
  - `git.rs` — status, log, diff, commit, push, pull, branch, checkout (9 CLI, 4 MCP tool)
  - `build.rs` — Rust/Node/Python/Go build + test runner (2 CLI, 2 MCP tool)
  - `deps.rs` — outdated + CVE taraması, cargo audit / npm audit (1 CLI, 1 MCP tool)
  - `env.rs` — .env vs .env.example diff, eksik/boş key tespiti (1 CLI, 1 MCP tool)
  - `version.rs` — semver bump, CHANGELOG.md üretimi, git tag (2 CLI, 2 MCP tool)
  - `process.rs` — port listesi, process listesi, kill-port (2 CLI, 1 MCP tool)
  - `disk.rs` — proje boyut analizi, cache temizleme (2 CLI, 1 MCP tool)
- **Aggregate MCP Tools:** `project_info` (tek çağrıda git+health+version+env+disk) + `portfolio_status` (42 proje özet tablosu). Önceki 5-8 tool çağrısı → 1 çağrıya düştü.
- **TUI Enhancements:** Proje detay paneli health grades + constitution issues + env flags gösteriyor. Health view'da `[c]` commit / `[p]` push kısayolları eklendi.
- **E2E Test:** 66/66 unit test yeşil. CLI smoke tests geçti. 23 MCP tool doğrulandı.
- **Toplam:** 32 CLI komutu, 23 MCP tool, 66 unit test

## Gemini
### Yaptıkları
- **SIGMAP Tracking:** R-AI-OS Health Dashboard'a ve SQLite (`health_cache`) veritabanına `has_sigmap` kolonları eklenerek tüm projelerin `sigmap` (imza haritası) durumu merkezi olarak takip edilebilir hale getirildi.
- **Project Versioning:** Projelere `memory.md` üzerinden otomatik sürüm ve nickname takibi desteği eklendi.
- **Self-Healing Loop:** `aiosd` üzerine `ValidationWorker` eklendi. `cargo check` ve compliance sonuçları MCP üzerinden raporlanabiliyor.
- **Architectural Memory:** `ask_architect` MCP aracı ile RAG tabanlı mimari danışmanlık katmanı eklendi.
- **Workspace Sync:** MASTER.md ve yollar `Dev_Ops_New` yapısına göre güncellendi.
- **UI Performance Fix:** All Projects ekranındaki takılma, senkronize I/O kaldırılarak ve cache kullanılarak giderildi.
- **Visual Grid Refactor:** All Projects ekranı modern bir `Table` yapısına kavuşturuldu.

## Antigravity
### Yaptıkları
- **Table-Based Health UI:** Dashboard listesi `ratatui::Table` ile yenilendi.
- **Binary Recovery:** Windows üzerindeki dosya kilitlenme sorunları süreç yönetimiyle çözüldü.
- **Refactor (High Priority):** 
  - `app/events.rs` içerisindeki kopya kod (dead code) temizlendi.
  - `daemon/server.rs` içerisindeki `Cortex::init().unwrap()` asenkron panik riski giderildi (hata yönetimi eklendi).
  - `app/mod.rs` içerisindeki `run_graphify` metodunda shell command injection zafiyeti giderildi.
  - `app/mod.rs` içerisindeki proje sıralama işlemlerindeki O(n²) filesystem I/O operasyonu engellenip cache üzerinden okunması sağlandı.

## Plan
### Tamamlananlar
- [x] SQLite Geçişi (Phase 1A).
- [x] Manifest Sistemi (Phase 1B).
- [x] Embedded Workers (Phase 2).
- [x] Event-driven Sentinel (Phase 3A).
- [x] Security + 22 Unit Tests (Phase 4).
- [x] v1.1.6 Visual Grid (Enhanced).
- [x] Refactor Scanner — health entegrasyonu.
- [x] v1.2.0 Core Toolkit — 7 modül, 32 CLI, 23 MCP tool.
- [x] project_info + portfolio_status aggregate MCP tools.
- [x] TUI — proje detay zenginleştirildi, health view git actions.
- [x] E2E test — 66/66 unit test, CLI smoke, 23 MCP tool.
### Devam Edenler
- [ ] 83-field AppState refactor (Phase 3B — Sub-states).
- [ ] portfolio_status status kolonunu DB'den düzgün çek (bazı projelerde memory.md içeriği karışıyor).
### Sıradakiler
- [ ] CI/CD durum takibi (GitHub Actions API).
- [ ] Health view'da build/test/deps kolonları.
- [ ] Proje detay görünümünde bağımlılık grafiği.

## Karar Günlüğü

| Tarih | Agent | Karar | Neden |
|-------|-------|-------|-------|
| 2026-05-08 | Claude | SQLite Persistence | JSON dosya yazımındaki race condition'ları önlemek ve O(1) sorgu performansı için. |
| 2026-05-08 | Claude | Embedded Workers | Uygulamanın daemon olmadan standalone çalışabilmesi için. |
| 2026-05-08 | Gemini | Non-blocking Render | UI thread'inin disk I/O bekleyerek donmasını engellemek için. |
| 2026-05-08 | Claude | Event-driven Sentinel | CPU yükünü azaltmak ve anlık tepki için. |
| 2026-05-14 | Claude | AI-free Core Toolkit | Raios'un AI API'sine bağımlı olmaması; her feature CLI + MCP olarak erişilebilir olsun diye. |
| 2026-05-14 | Claude | project_info aggregate tool | Agent'ların 5-8 tool yerine 1 çağrıyla tüm proje bilgisine erişmesi için (~5x token tasarrufu). |
| 2026-05-14 | Claude | 66 unit test baseline | Her modülün izole test edilebilmesi; regresyon tespiti için CI hazırlığı. |
