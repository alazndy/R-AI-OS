# R-AI-OS Memory

## Son Durum
- **Version:** v1.1.0
- **Status:** Stabil. 6 yeni özellik + 3 kritik fix tamamlandı.
- **Aktif agentlar:** Claude Code
- **Durum:** Binary `.cargo/bin` ve `.aios` dizinlerine deploy edildi.

## Claude
### Yaptıkları
- **v1.0.0 Features (6 özellik):**
  - `raios commit [--push] [--dry-run]` — toplu dirty proje commit
  - `raios stats [--json]` — portfolio grade/dirty/kategori istatistikleri
  - `raios new <name> [--category] [--github]` — MASTER kurallarına uygun proje scaffold
  - Dashboard Quick Stats widget (grade %, dirty, local-only sayıları)
  - All Projects sort (`s` tuşu): Name / Grade / Dirty / Category / Status
  - MCP: `get_health`, `list_projects`, `get_stats` yeni toollar
- **v1.1.0 Fixes (3 fix):**
  - `app/ipc.rs`: `println!` kaldırıldı, retry loop eklendi (8s, max 10 deneme)
  - `mempalace.rs`: WalkDir max_depth(4) → 2-level akıllı scan (monorepo içleri artık proje sayılmıyor)
  - `entities.rs`: discover artık scanner'ı kaynak kabul ediyor, ghost kayıtlar otomatik temizleniyor (98 → 40 proje)

### Yapacakları
- [ ] `raios stats` çalışma süresi optimizasyonu (health check paralel yapılabilir)
- [ ] Dashboard stats widget'ı daemon bağlıyken otomatik yenile

### Notlar
- `09_Archive/akort-legacy-archive` monorepo olduğu için tek proje olarak sayılıyor (doğru davranış)
- `raios discover` sonrası entities.json ghost kayıtlardan temizleniyor

## Gemini
### Yaptıkları
- v0.9.0: GitHub Remote URL desteği, 90 projeyi CLEAN'e çekme, memory.md standardizasyonu
### Yapacakları
- [ ] GitHub Sync: entities.json ↔ remote repo verileri periyodik eşleme

## Plan
### Tamamlananlar
- [x] v0.9.0: GitHub Remote URL + Workspace Auto-Cleanup
- [x] v1.0.0: CLI Power Tools (commit/stats/new) + TUI Sort + MCP Tools
- [x] v1.1.0: IPC reconnect loop, mempalace derinlik fix, entities prune

### Devam Edenler
- [ ] —

### Sıradakiler
- [ ] Parallel health check (`raios stats` hız optimizasyonu)
- [ ] SQLite migration (entities.json → SQLite)
- [ ] Cron/scheduled maintenance agents

## Karar Günlüğü
| Tarih | Agent | Karar | Neden |
|-------|-------|-------|-------|
| 2026-05-07 | Gemini | Akort Consolidation | GitHub'daki kalabalığı azaltmak |
| 2026-05-07 | Claude | 6 yeni özellik (v1.0.0) | CLI araçları + TUI + MCP |
| 2026-05-07 | Claude | mempalace 2-level scan | 98 hayalet proje → 40 gerçek proje |
| 2026-05-07 | Claude | IPC retry loop | TUI'ye println! basılıyordu, bağlantı tek seferdi |
