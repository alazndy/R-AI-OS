# R-AI-OS Memory

## Son Durum
- **Version:** v1.1.0 (build: 2026-05-07)
- **Status:** Setup Wizard tamamlandı. Sıfırdan tam kurulum artık destekleniyor.
- **Aktif agentlar:** Claude Code
- **Durum:** `.aios/raios.exe` güncellendi. `.cargo/bin` terminal yeniden başlatınca güncellenir.

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
  - `mempalace.rs`: WalkDir max_depth(4) → 2-level akıllı scan (98 → 40 proje)
  - `entities.rs`: discover scanner'ı kaynak alıyor, ghost kayıtlar temizleniyor
- **v1.1.0 Setup Wizard (8 adım):**
  - `src/setup_wizard.rs` — agent detect, workspace scaffold, template üretimi
  - `src/ui/setup.rs` — progress gauge + sistem tarama + directory preview + action log
  - Adımlar: Welcome → Workspace → MASTER.md → Claude → Gemini → Antigravity → Skills → Initialize → Done
  - Claude: `~/.claude/CLAUDE.md` + MCP `settings.json` kaydı + `rules/` dizini
  - Gemini: `~/.gemini/GEMINI.md` + MCP `settings.json` kaydı
  - Antigravity: `.agents/ANTIGRAVITY.md`
  - Skills: `prompt-master`, `graphify`, `verify-ai-os`, `ki-snapshot` stub'ları
  - `[Tab]` → ajanı atla/geri al, `[s]` → adım ilerle, `[Enter]` → düzenle/onayla

### Yapacakları
- [ ] `raios stats` paralel health check (hız optimizasyonu)
- [ ] Dashboard stats widget daemon bağlıyken otomatik yenile
- [ ] Versiyon `1.2.0`'a bump (wizard major feature)

### Notlar
- `09_Archive/akort-legacy-archive` monorepo → tek proje (doğru)
- Config yoksa wizard otomatik açılır (fresh install deneyimi)
- MCP kaydı `settings.json`'a JSON merge ile yapılıyor, mevcut config bozulmuyor

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
- [x] v1.1.0: 8-adımlı Zero-Setup Wizard (Claude+Gemini+Antigravity+Skills+MCP)

### Devam Edenler
- [ ] —

### Sıradakiler
- [ ] `raios stats` parallel health check
- [ ] SQLite migration (entities.json → SQLite)
- [ ] Cron/scheduled maintenance agents
- [ ] `raios setup --force` komutu (wizard'ı manuel tetikleme)

## Karar Günlüğü
| Tarih | Agent | Karar | Neden |
|-------|-------|-------|-------|
| 2026-05-07 | Gemini | Akort Consolidation | GitHub'daki kalabalığı azaltmak |
| 2026-05-07 | Claude | 6 yeni özellik (v1.0.0) | CLI araçları + TUI + MCP |
| 2026-05-07 | Claude | mempalace 2-level scan | 98 hayalet proje → 40 gerçek proje |
| 2026-05-07 | Claude | IPC retry loop | TUI'ye println! basılıyordu |
| 2026-05-07 | Claude | 8-adım Setup Wizard | Sıfır config'den tam kurulum talebi |
