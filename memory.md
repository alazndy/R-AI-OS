# R-AI-OS Memory

## Son Durum
- **Version:** v1.1.0 (build: 2026-05-07)
- **Status:** OWASP Security Scanner eklendi. Her proje güvenlik skoru alıyor.
- **Aktif agentlar:** Claude Code
- **Durum:** `.aios/raios.exe` güncellendi. R-AI-OS kendi kendini taradı → B (85/100).

## Claude
### Yaptıkları
- **v1.0.0 Features (6 özellik):**
  - `raios commit [--push] [--dry-run]` — toplu dirty proje commit
  - `raios stats [--json]` — portfolio grade/dirty/kategori istatistikleri
  - `raios new <name> [--category] [--github]` — MASTER kurallarına uygun proje scaffold
  - Dashboard Quick Stats widget
  - All Projects sort (`s` tuşu)
  - MCP: `get_health`, `list_projects`, `get_stats`
- **v1.1.0 Fixes + Features:**
  - IPC retry loop (8s, max 10 deneme), `println!` kaldırıldı
  - mempalace 2-level scan (98 → 40 proje)
  - entities.rs ghost kayıt temizleme
  - 8-adımlı Zero-Setup Wizard (Claude+Gemini+Antigravity+Skills+MCP)
- **v1.1.0 OWASP Security Scanner:**
  - `src/security.rs` — 20 OWASP pattern (A01-A09), bağımlılık audit, skor sistemi
  - `raios security [--project] [--full] [--path] [--json]`
  - Tarama kapsamı: hardcoded secret/password, MD5/SHA1, SQL injection, eval/innerHTML,
    command injection, DEBUG=True, CORS wildcard, JWT none, .env git'te, console.log sızıntısı
  - Dil desteği: Rust, Python, TypeScript, JavaScript, Go, YAML, ENV
  - Bağımlılık denetimi: `pnpm audit`, `cargo audit`, `pip-audit`
  - Skor: 100 taban, Critical -25, High -15, Medium -10, Low -5
  - Grade: A(≥90) B(≥75) C(≥50) D(≥25) F(<25)
  - Health dashboard: her projede `🔒85B` güvenlik kolonu
  - `ProjectHealth`'e `security_score`, `security_grade`, `security_critical` eklendi

### Yapacakları
- [ ] `raios stats` paralel health check (hız optimizasyonu)
- [ ] Versiyon `1.2.0`'a bump
- [ ] `raios setup --force` komutu
- [ ] Security scan sonuçlarını cache'le (her sorguda yeniden taramak yavaş)

### Notlar
- `09_Archive/akort-legacy-archive` monorepo → tek proje (doğru)
- Config yoksa wizard otomatik açılır
- MCP kaydı `settings.json`'a JSON merge ile yapılıyor
- `regex-lite = "0.1"` bağımlılığı eklendi

## Gemini
### Yaptıkları
- v0.9.0: GitHub Remote URL desteği, 90 projeyi CLEAN'e çekme, memory.md standardizasyonu
### Yapacakları
- [ ] GitHub Sync: entities.json ↔ remote repo verileri periyodik eşleme

## Plan
### Tamamlananlar
- [x] v0.9.0: GitHub Remote URL + Workspace Auto-Cleanup
- [x] v1.0.0: CLI Power Tools (commit/stats/new) + TUI Sort + MCP Tools
- [x] v1.1.0: IPC fix, mempalace fix, entities prune
- [x] v1.1.0: 8-adımlı Zero-Setup Wizard
- [x] v1.1.0: OWASP Security Scanner (20 pattern + audit + skor)

### Devam Edenler
- [ ] —

### Sıradakiler
- [ ] Security scan cache (hız)
- [ ] `raios stats` parallel health check
- [ ] SQLite migration (entities.json → SQLite)
- [ ] Cron/scheduled maintenance agents
- [ ] `raios setup --force`

## Karar Günlüğü
| Tarih | Agent | Karar | Neden |
|-------|-------|-------|-------|
| 2026-05-07 | Gemini | Akort Consolidation | GitHub'daki kalabalığı azaltmak |
| 2026-05-07 | Claude | 6 yeni özellik (v1.0.0) | CLI araçları + TUI + MCP |
| 2026-05-07 | Claude | mempalace 2-level scan | 98 hayalet proje → 40 gerçek proje |
| 2026-05-07 | Claude | IPC retry loop | TUI'ye println! basılıyordu |
| 2026-05-07 | Claude | 8-adım Setup Wizard | Sıfır config'den tam kurulum talebi |
| 2026-05-07 | Claude | OWASP Security Scanner | Her projenin güvenlik skoru olsun talebi |
