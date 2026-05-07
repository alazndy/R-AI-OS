# R-AI-OS Memory

## Son Durum
- **Tarih:** 2026-05-07
- **Sürüm:** v1.1.4 (Ghost Protocol)
- **Aktif agentlar:** Claude Code + Antigravity
- **Durum:** Ghost Protocol yayında. Otomatik daemon spawn, hibrit arama (BM25 + semantic), README overhaul ve NotebookLM export tamamlandı. Sonraki sürüm kararı bekleniyor.

## Claude
### Yaptıkları
- **v1.0.0:** `raios commit`, `raios stats`, `raios new`, Dashboard Quick Stats, All Projects sort, MCP tools (get_health, list_projects, get_stats)
- **v1.1.0:** IPC retry loop, mempalace 2-level scan (98→40 proje), entities ghost prune, 8-adımlı Zero-Setup Wizard (Claude+Gemini+Antigravity+Skills+MCP), OWASP Security Scanner (20 pattern, bağımlılık audit, skor sistemi)
### Yapacakları
- [ ] Seçilen v1.1.5 özelliğini implement et (Antigravity karar günlüğüne bak)
### Notlar
- OWASP scanner: regex-lite bağımlılığı, 20 pattern, Critical/High/Medium/Low skor
- Security CLI: `raios security [--project] [--full] [--path] [--json]`
- Health dashboard: 🔒 güvenlik kolonu eklendi

## Gemini
### Yaptıkları
- v0.9.0: GitHub Remote URL desteği, 90 projeyi CLEAN'e çekme, memory.md standardizasyonu
### Yapacakları
- [ ] GitHub Sync: entities.json ↔ remote repo verileri periyodik eşleme

## Antigravity
### Yaptıkları
- **README Overhaul:** Ürün odaklı, profesyonel İngilizce README hazırlandı ve GitHub'a push edildi.
- **NotebookLM Export:** Tüm kaynak kodu (`.rs`, `.toml`, `.md`, `.json`, `.yaml`) Markdown blokları içine sarılarak `C:\Users\turha\Desktop\RAIOS_Source_NotebookLM` klasörüne paketlendi. Python tabanlı `export_for_notebook.py` otomasyonu yazıldı.
- **Auto-Spawn Daemon:** `ipc.rs`'e `ensure_daemon_running()` eklendi — `aiosd` yoksa TUI açılışında arka planda otomatik başlatılıyor (Windows: `CREATE_NO_WINDOW`).
- **Hibrit Arama:** `hybrid_search.rs` — BM25 + fastembed semantic arama birleştirildi. `raios search <query>` CLI komutu eklendi.
- **Mempalace Recursive Scan:** `mempalace.rs` yeniden yazıldı — derinlik 4'e kadar recursive tarama, proje kök tespiti güçlendirildi (`src/app/lib/scripts` kontrolü eklendi).
- **v1.1.4 Ghost Protocol:** Versiyon güncellendi, git hardening yapıldı.
### Yapacakları
- [ ] v1.1.5 seçeneğinin implementasyonu
### Notlar
- `fastembed = "3"` ve `instant-distance = "0.6"` Cargo.toml'a eklendi
- `lib.rs`'e `cortex` ve `hybrid_search` modülleri eklendi
- `cli.rs`'e `Search` subcommand eklendi (`--top-k`, `--reindex`)

## Plan
### Tamamlananlar
- [x] v0.9.0: GitHub Remote URL + Workspace Auto-Cleanup
- [x] v1.0.0: CLI Power Tools + TUI Sort + MCP Tools
- [x] v1.1.0: IPC fix, mempalace fix, entities prune
- [x] v1.1.0: 8-adımlı Zero-Setup Wizard
- [x] v1.1.0: OWASP Security Scanner
- [x] v1.1.4: Auto-Spawn Daemon + Hibrit Arama + README + NotebookLM Export

### Devam Edenler
- [ ] v1.1.5 seçimi ve implementation

### Sıradakiler (3 Seçenek — Karar Bekleniyor)
- [ ] **Seçenek 1 — Self-Healing Loop:** `cargo check` + test + `raios security` otomatik tetikleme, hata durumunda ajana geri fırlatma
- [ ] **Seçenek 2 — Architectural Memory:** `cortex` modülü fonksiyon/kural/karar vektörel bağlantı, MASTER.md reasoning
- [ ] **Seçenek 3 — Local Sovereignty:** Ollama/fastembed ile yerel model entegrasyonu, internet olmadan otonom çalışma
- [ ] SQLite migration (entities.json → SQLite)
- [ ] `raios setup --force` komutu

## Karar Günlüğü
| Tarih | Agent | Karar | Neden |
|-------|-------|-------|-------|
| 2026-05-07 | Gemini | Akort Consolidation | GitHub'daki kalabalığı azaltmak |
| 2026-05-07 | Claude | 6 yeni özellik (v1.0.0) | CLI araçları + TUI + MCP |
| 2026-05-07 | Claude | mempalace 2-level scan | 98 hayalet proje → 40 gerçek proje |
| 2026-05-07 | Claude | IPC retry loop | TUI'ye println! basılıyordu |
| 2026-05-07 | Claude | 8-adım Setup Wizard | Sıfır config'den tam kurulum talebi |
| 2026-05-07 | Claude | OWASP Security Scanner | Her projenin güvenlik skoru olsun talebi |
| 2026-05-07 | Antigravity | Auto-Spawn Daemon | Kullanıcının daemon'ı ayrı başlatma yükünü ortadan kaldırmak |
| 2026-05-07 | Antigravity | Hibrit Arama (BM25+Semantic) | Bağlam odaklı arama, sadece dosya adı değil anlam |
| 2026-05-07 | Antigravity | Ghost Protocol v1.1.4 | README + NotebookLM export + versiyon bump |
| 2026-05-07 | Antigravity | 3 Seçenek sunuldu | Self-Healing / Arch Memory / Local Sovereignty — karar bekleniyor |
