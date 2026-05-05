# R-AI-OS Memory

## R-AI-OS Memory
- **Version:** v0.2.3 (Stable)
- **Status:** Active Development
- **Aktif agentlar:** Claude Code + Antigravity
- **Durum:** Graphify Entegrasyonu ve Discovery Engine tamamlandı. Sistem PATH'e kaydedildi ('raios' command active). TUI full-featured.

## Claude
### Yaptıkları
- Proje iskeleti ve temel TUI (ratatui 0.29 + crossterm 0.28) kurulumu.
- `compliance.rs`: Kod kalite denetçisi — Rust/TS/Python/package.json kuralları, secret detection.
- `indexer.rs`: BM25 tabanlı yerel arama motoru, tüm Dev Ops dosyalarını indexliyor.
- `health.rs` + `entities.rs`: Multi-project sağlık takibi, entities.json parser.
- `mempalace.rs`: Dev Ops filesystem scan → oda/proje ağacı, memory.md status extraction.
- **Neural Context / Semantic Memory**: `/search <query>` → BM25 search, Neural Search ekranı.
- **Master Rule Enforcement**: Dosya açılınca compliance badge `📊 87/100 B [Rust] [3 issues]`.
- **Tüm AI agent kural dosyaları discovery**: Claude, Gemini, Antigravity, Cursor, Windsurf, Copilot, Jules.
- **Syntax highlighting**: `.md`, `.rs`, `.ts`, `.py`, `.toml`, `.yaml` — line-level renklendirme.
- **entities.json entegrasyonu**: All Projects menüsü, proje listesi kategori+status ile.
- **Git status overlay**: Recent projects'te `● dirty / ○ clean` + branch adı.
- **`/memo` quick note**: `Dev Ops/_session_notes.md`'ye timestamp'li append.
- **Proje detay ekranı** (`AppState::ProjectDetail`): memory.md + git log, `[Tab]` panel switch, `[e]` edit.
- **Command Palette**: `/` veya Tab → fuzzy-match overlay, `[↑↓]` nav, `[Tab]` fill, `[Enter]` run.
- **Multi-Project Health Dashboard** (`/health`): compliance + git + constitution diff tüm projeler.
- **File Watcher**: Açık dosya dışarıdan değişince `⚡ File changed — [R] reload` badge.
- **Constitution Diff**: MASTER.md'ye karşı proje CLAUDE.md kontrolü (pnpm, RLS, api_key, skills).
- **Agent Session Launcher**: `[L]` → Claude/Gemini'yi proje dizininde yeni terminalde aç.
- **MemPalace tam ekran** (`AppState::MemPalaceView`, `/mempalace`): filesystem'den 48+ proje, oda/proje ağacı, memory.md status preview.
- **Command bug fix**: `handle_command_key` Enter handler rewrite — Tab+navigate+Enter artık çalışıyor.

### Yapacakları
- [ ] WebSocket daemon bağlantısı (aiosd ile)
- [ ] Mouse event desteği
- [ ] GitHub repo oluştur ve ilk commit at
- [ ] entities.json'ı otomatik güncelle (yeni proje kurulunca)

### Notlar
- ratatui 0.29 + crossterm 0.28 + serde_json + chrono 0.4 + walkdir 2
- Custom line editor (tui-textarea bağımlılığından kaçınıldı)
- AppState'ler: Booting, Setup, Dashboard, FileView, FileEdit, ProjectDetail, HealthView, Search, MemPalaceView
- CLI: `raios rules`, `raios memory <proje>`, `raios view <dosya>`
- Commands: `/sync`, `/health`, `/search`, `/mempalace`/`/mp`, `/open`, `/memo`, `/reindex`, `/quit`

## Antigravity
### Yaptıkları
- **Dinamik Konfigürasyon**: `config.rs` ile `~/.config/raios/config.toml` üzerinden yol yönetimi.
- **Setup Wizard**: İlk çalıştırmada otomatik dizin algılama ve bağımlılık kontrolü.
- **Master Installer**: `install-system.ps1` ile 0'dan sistem kurulumu ve PATH kaydı.
- **Input Optimization**: Windows terminalindeki çift event/lag sorunu giderildi.
- **Discovery Refactor**: Hardcoded yollar temizlendi, config odaklı mimari.
- **AppState::Search** ekledi: Ctrl+P ile fuzzy search overlay.
- **Activity/Log altyapısı**: `Activity` ve `LogEntry` struct'ları (BgMsg'de hazır).
- **Graphify Entegrasyonu**:
    - **Motor**: `run_graphify` ile projeye özgü Python grafik analizi tetikleme.
    - **Health Dashboard**: Multi-project Graphify status (`✓graph` / `✗graph`).
    - **Project Detail**: Graphify ready/pending status + `[G]` hızlı çalıştırma kısayolu.
    - **TUI Markdown Previewer**: `[R]` ile `GRAPH_REPORT.md` dosyasını uygulama içinden görüntüleme (scrolling + highlighting).
    - **Discovery**: `Dev Ops/AI OS/graphify` altındaki betiğin otomatik tespiti.
- **Project Discovery Engine**:
    - `entities.rs`: `discover_entities` ile tüm Dev Ops dizinini (Rooms/Projects) tarayıp `entities.json` ile birleştirme.
    - `/discover` komutu: Eksik projeleri bulur ve `entities.json` dosyasını kalıcı olarak günceller.
    - Startup Auto-Scan: Uygulama açılırken artık tüm projeleri (120+) otomatik olarak listeler.
    - **Refinement**: Root-level projeler (Crucix, Portfolio vb.) artık `min_depth(0)` ile doğru tespit ediliyor. Kriterler build dosyalarıyla (Cargo, Go, Python) rafine edildi.
- **Scrolling Fix:** MemPalace ve All Projects listelerinde 120+ proje için kaydırma mantığı düzeltildi.
- **Memory Detection:** `memory.md` tespiti için case-insensitivity ve `.agents/memory.md` desteği eklendi.
- Flattened line calculation ile imleç takibi sağlandı.

### Yapacakları
- [ ] **Agent Handoff**: `raios` üzerinden Claude/Gemini'ye doğrudan görev paslama (Unified Shell)
- [ ] Mouse desteği ekle.

## Plan
### Tamamlananlar
- [x] Proje iskeleti (Cargo, src/ yapısı)
- [x] Boot screen + Setup Wizard
- [x] Dashboard (8 menü öğesi)
- [x] File viewer (satır numaralı, syntax highlighted)
- [x] File editor (custom cursor + Ctrl+S)
- [x] Dinamik Config + auto-detect
- [x] Sistem Gereksinim Denetleyicisi (Setup'ta [CRITICAL] tag)
- [x] Compliance Engine (dosya bazlı kural analizi)
- [x] Neural Search (BM25 indexer)
- [x] Agent Rule File Discovery (7 agent tipi)
- [x] entities.json parser + All Projects view
- [x] Git Status Overlay (recent projects)
- [x] Project Detail Screen (memory.md + git log)
- [x] Health Dashboard (/health)
- [x] Command Palette (fuzzy overlay)
- [x] File Watcher (mtime polling, reload badge)
- [x] Constitution Diff (per-project MASTER.md check)
- [x] Agent Session Launcher ([L] → new terminal)
- [x] MemPalace full-screen view (/mempalace)
- [x] Syntax Highlighting (md/rs/ts/py/toml/yaml)
- [x] /memo quick note
- [x] Command bug fix (Tab+Enter artık çalışıyor)
- [x] Graphify Motor Entegrasyonu ([G] -> subprocess wt/cmd)
- [x] Graphify Health Dashboard (multi-project status)
- [x] Graphify Markdown Previewer ([R] -> TUI viewer with scroll)
- [x] Project Discovery Engine (120+ proje otomatik tespiti)
- [x] /discover komutu ve entities.json kalıcı güncelleme
- [x] v0.2.1 (Refined Discovery): Root-level projeler ve build-file tabanlı filtreleme.

### Devam Edenler
- [ ] Fuzzy Search UI Entegrasyonu (Ctrl+P — Antigravity tamamlıyor)

### Sıradakiler
- [ ] Context Injection: Graphify raporlarını LLM context'ine otomatik basacak pipeline
- [ ] WebSocket Daemon entegrasyonu (aiosd)
- [ ] Unified Agent Shell
- [ ] Mouse event desteği
- [ ] GitHub push

## Karar Günlüğü
| Tarih | Agent | Karar | Neden |
|-------|-------|-------|-------|
| 2026-05-04 | Claude | tui-textarea yerine custom editor | Bağımsızlık ve hafiflik |
| 2026-05-04 | Antigravity | Config-first mimari | Portabilite ve kullanıcı dostu ilk kurulum |
| 2026-05-04 | Antigravity | Windows KeyEventKind::Press filtresi | Windows terminalindeki input bug |
| 2026-05-04 | Claude | tokio yerine std::thread + mpsc | ratatui sync loop ile uyum, sade mimari |
| 2026-05-04 | Claude | serde_yaml yerine filesystem scan | mempalace.yaml'ı parse etmek yerine direkt dizin taramak daha güvenilir |
| 2026-05-04 | Claude | Command Enter rewrite | chosen.starts_with() koşulu Tab modunda cmd'yi yanlış resolve ediyordu |
| 2026-05-05 | Antigravity | Graphify TUI Previewer | Raporlar için dış viewer yerine iç Markdown render tercih edildi |
| 2026-05-05 | Antigravity | Discovery Engine | entities.json'daki eksik projelerin otomatik tespiti sağlandı |
| 2026-05-05 | Antigravity | min_depth(0) & build-file markers | Root-level projelerin (Crucix vb.) tespiti ve 120+ gerçek proje doğrulaması |
