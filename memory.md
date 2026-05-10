# R-AI-OS Memory

## Son Durum
- Tarih: 2026-05-08
- Aktif agent: Gemini (Documentation Sync)
- Sürüm: v1.1.6
- Sürüm Adı: Visual Grid (Enhanced)
- Durum: Tüm sistem mimari olarak SQLite tabanlı yeni yapıya taşındı. Manifest sistemi, gömülü worker'lar ve yüksek test kapsama oranıyla kararlılık sağlandı.

## Claude
### Yaptıkları
- **Faz 1A: SQLite Migration:** `entities.json` yapısı tamamen `rusqlite` tabanlı SQLite veritabanına taşındı. İstatistik hesaplamaları O(1) hızına düşürüldü.
- **Faz 1B: Manifest System:** `.raios.yaml` manifest desteği eklendi. Scanner artık projeleri bu dosya üzerinden öncelikli olarak tanıyor. `raios new` ile otomatik scaffold oluşturma özelliği eklendi.
- **Faz 2: Embedded Workers:** `workers.rs` modülü ile `aiosd` (daemon) çalışmıyorsa bile ana uygulama üzerinden gömülü worker'ların (health, scanner vb.) çalışması sağlandı. Tek binary çalışma yeteneği kazandırıldı.
- **Faz 3A: Event-driven Sentinel:** Polling tabanlı kontrol yerine `notify` kütüphanesiyle olay güdümlü (event-driven) yapıya geçildi. 600ms debounce ile performans optimize edildi.
- **Faz 4: Security & Testing:** Semgrep tabanlı ek güvenlik katmanı eklendi. Proje keşfindeki derinlik hataları giderildi. 22 adet unit test yazıldı ve başarıyla doğrulandı.
- **Bug Fix:** `static_scan` sırasında `.tmp` ile başlayan geçici klasörlerin taranmama sorunu giderildi.

## Gemini
### Yaptıkları
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

## Plan
### Tamamlananlar
- [x] SQLite Geçişi (Phase 1A).
- [x] Manifest Sistemi (Phase 1B).
- [x] Embedded Workers (Phase 2).
- [x] Event-driven Sentinel (Phase 3A).
- [x] Security + 22 Unit Tests (Phase 4).
- [x] v1.1.6 Visual Grid (Enhanced) yayında.
### Devam Edenler
- [ ] 83-field AppState refactor (Phase 3B - Sub-states).
- [ ] Self-healing için Python/JS linter destekleri.
### Sıradakiler
- [ ] Dashboard üzerinden doğrudan Git aksiyonları (Commit, Push).
- [ ] Proje detay görünümünde bağımlılık grafiği.

## Karar Günlüğü
| Tarih | Agent | Karar | Neden |
|-------|-------|-------|-------|
| 2026-05-08 | Claude | SQLite Persistence | JSON dosya yazımındaki yarış durumlarını (race conditions) önlemek ve O(1) sorgu performansı için. |
| 2026-05-08 | Claude | Embedded Workers | Uygulamanın daemon olmadan da (standalone) tam fonksiyonel çalışabilmesi için. |
| 2026-05-08 | Gemini | Non-blocking Render | UI thread'inin disk I/O bekleyerek donmasını engellemek için. |
| 2026-05-08 | Claude | Event-driven Sentinel | CPU yükünü azaltmak ve değişikliklere anlık tepki verebilmek için. |
