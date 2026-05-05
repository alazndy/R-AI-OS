# R-AI-OS Memory

## R-AI-OS Memory
- **Version:** v0.2.6 (Stable)
- **Status:** **Task → Agent Dispatch** eklendi. Task panelinden [c]/[g]/[a] ile ajan yönlendirme aktif.
- **Aktif agentlar:** Claude Code + Antigravity
- **Durum:** `raios` CLI v0.2.6: task dispatch (@agent, #project tag sistemi), clipboard+terminal launch.

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
- **Unified Agent Shell:** MemPalace ve All Projects listelerinde `[C]`, `[G]`, `[A]` tuşlarıyla Claude, Gemini ve Antigravity ajanlarını doğrudan proje klasöründe başlatma yeteneği.
- **Real-time Activity Ticker:** Dashboard'un altında sistem olaylarını (hata, uyarı, ajan başlatma) gösteren dinamik bir alt bar.
- **Timeline & Live Logs:** Sistem genelindeki olayların tarihsel takibi için iki yeni Dashboard sekmesi.
- **Scrolling Fix:** MemPalace ve All Projects listelerinde 120+ proje için kaydırma mantığı düzeltildi.
- **Memory Detection:** `memory.md` tespiti için case-insensitivity ve `.agents/memory.md` desteği eklendi.
- Flattened line calculation ile imleç takibi sağlandı.
- **Task Management Integration:**
    - `tasks.md` parser eklendi. Dashboard sağ panelinde görev listesi görüntülenebilir.
    - `/task add <metin>` komutu ve Space/X ile tamamlama (auto-save) desteği.
- **Vault ↔ R-AI-OS Bridge:**
    - Obsidian `Vault101` entegrasyonu sağlandı.
    - `/vault-create <proje>` komutu ile otomatik frontmatter'lı `.md` notu oluşturma.
    - Proje listesinde Vault notu durumu (`[V]` / `[-]`) badge olarak eklendi.
- **Real-time Port Monitor:**
    - Header ticker'a aktif port takibi eklendi (3000, 5173, 8080, 4200).
    - `TcpStream::connect_timeout` ile non-blocking port tarama.
- **CLI & UX:**
    - `raios --version` (clap default) ve `raios version` (custom subcommand) desteği.
    - `MASTER.md` dosyası "Policies" sekmesinden "Constitution" sekmesine (Rules) taşındı.
    - Config yolu `Vault101` olarak güncellendi.

- [ ] Mouse desteği ekle (Wheel scroll & Click selection).
- [ ] GitHub Sync: entities.json ile remote repo verilerini otomatik eşleme.

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
- [x] Unified Agent Shell ([C], [G], [A] keys)
- [x] Real-time Activity Ticker (Bottom bar)
- [x] Timeline & Live Logs views
- [x] /discover komutu ve entities.json kalıcı güncelleme
- [x] v0.2.1 (Refined Discovery): Root-level projeler ve build-file tabanlı filtreleme.
- [x] Task Management Integration (tasks.md parser & UI)
- [x] Vault ↔ R-AI-OS Bridge (/vault-create)
- [x] Real-time Port Monitor (Header ticker)
- [x] v0.2.5: CLI Versioning & MASTER.md reorganization

### Devam Edenler
- [ ] Fuzzy Search UI Entegrasyonu (Ctrl+P — Antigravity tamamlıyor)

### Sıradakiler
- [ ] **v0.2.9 (Acil):** `memory.md.lock` (File Mutex) mekanizması. Race condition önleme.
- [ ] **v0.3.0 (Mimari):** **aiosd** (Tokio-based Daemon) geçişi. State yönetimini arka plana taşıma.
- [ ] **Altyapı:** `notify` crate entegrasyonu. Polling yerine event-driven compliance scan.
- [ ] **Git:** "Review-then-Push" flow (Onaysız otomatik push yasaklandı).

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
| 2026-05-05 | Antigravity | Task & Vault Bridge | Dev Ops/tasks.md ve Obsidian Vault entegrasyonu ile dokümantasyon odaklı gelişim |
| 2026-05-05 | Antigravity | Port Monitor Ticker | Web geliştirme süreçleri için port doluluğunu TUI header'da anlık görme |
| 2026-05-05 | Antigravity | Constitution (MASTER.md) | MASTER.md dosyasının daha görünür olması için Policies'den Rules sekmesine taşınması |
| 2026-05-05 | Antigravity | v0.2.5 Bump | CLI versioning ve stabilite iyileştirmeleri sonrası versiyon yükseltme |
| 2026-05-06 | Antigravity | Mouse Support İPTAL | Vim-like hız odağı ve farenin konsept dışı kalması |
| 2026-05-06 | Antigravity | aiosd Önceliklendirme | TUI/State ayrımı ve güvenilir arka plan yönetimi ihtiyacı |
| 2026-05-06 | Antigravity | memory.md.lock | Multi-agent race condition riskini minimize etmek |
