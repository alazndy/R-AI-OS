# R-AI-OS Memory

## Son Durum
- **Version:** v0.8.0 (Stable)
- **Status:** **Agent Execution Proxy**, **Bouncing Limit** ve `aiosd` daemon (IPC 42069) devrede.
- **Aktif agentlar:** Claude Code + Antigravity + Jules
- **Durum:** `Dev_Ops_New` HQ geçişi tamamlandı. Git diff inbox, multi-agent handover ve TUI güncellendi.

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

- [x] **v0.2.9:** `memory.md.lock` (File Mutex) mekanizması. Race condition önlendi.
- [x] **Altyapı:** `notify` crate entegrasyonu. Polling yerine event-driven file watcher yapısı kuruldu.
- [x] **v0.3.0:** MCP Stdio Server (`raios mcp-server`).
- [x] **v0.4.0:** Agent Execution Proxy (`raios run <agent>`). Environment izolasyonu ve Death Timer eklendi.
- [x] **v0.5.0:** Bouncing Limit TUI uyarısı (3 handover limiti) ve Git Diff Onay Ekranı (`D` tuşu ile).
- [x] Fuzzy Search UI Entegrasyonu (Ctrl+P)
- [x] **Compliance Refinement:** app.rs içinde FileChanged event'ini sadece hedeflenen dosya için compliance tetikleyecek şekilde optimize etme.

### Devam Edenler
- [x] **v0.6.0 (Mimari):** **aiosd** (Tokio-based Daemon) geçişi. State yönetimini arka plana taşıma.
    - [x] Bin split (raios, aiosd)
    - [x] Shared library extraction (r-ai-os)
    - [x] TCP/IPC Server (127.0.0.1:42069)
    - [x] Background File Watcher migration to Daemon
    - [x] Background BM25 Indexer migration to Daemon
    - [x] Bidirectional Communication (Search commands over TCP)
    - [x] Health Scanner migration to Daemon
- [ ] GitHub Sync: Sync entities.json with remote repository data (commit count, stars, etc.)
- [ ] Client/Daemon State Synchronization Refinement

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
| 2026-05-06 | Antigravity | v0.4 Agent Proxy | Ajanları child process olarak Env izoleli başlatma (api key güvenliği için) |
| 2026-05-06 | Antigravity | v0.5 Bouncing Limit & Diff | Ajanların sonsuz döngüsünü kırma (limit=3) ve İnsan onaylı Git akışı (D tuşu) |
| 2026-05-06 | Antigravity | memory.md.lock | Multi-agent race condition riskini minimize etmek |
| 2026-05-06 | Antigravity | Compliance Refinement | FileChanged event'inde aktif dosya için otomatik compliance reload eklendi |
| 2026-05-06 | Antigravity | Fuzzy Search UI | Ctrl+P Fuzzy Search popup'ının UI entegrasyonu tamamlandığı doğrulandı |
| 2026-05-06 | Antigravity | aiosd Daemon Architecture | Background tasks (Indexer, Watcher) decoupled from TUI for stability |
| 2026-05-06 | Antigravity | TCP-based IPC | JSON messages over 127.0.0.1:42069 for cross-process communication |
| 2026-05-06 | Antigravity | Neural Index on Daemon | Index building moved to daemon to prevent TUI startup lag |
| 2026-05-06 | Antigravity | Port Monitor and Health Scanner | Moved from TUI background thread to aiosd background tasks and implemented JSON payload responses |

<!-- MCP update by antigravity at 2026-05-06 02:28 -->
- [2026-05-06 02:28] **Refactored TUI application and UI monolithic files into modular components.**: Extracted the monolithic `app/mod.rs` into `state.rs`, `editor.rs`, `ipc.rs`, and `events.rs`. Extracted `ui/mod.rs` into specialized submodules for `dashboard`, `projects`, `health`, `mempalace`, `search`, `filebrowser`, `setup`, and `components`. All code compiles successfully with 0 errors. Committed changes to git for backup. Ready to start developing the Agent Execution Proxy.

## Antigravity
### Yaptıkları
- **HQ Migration:** Tüm sistemin ana üssü `Dev_Ops_New` olarak güncellendi.
- **Config Update:** `AppData/Roaming/raios/config.toml` dosyası yeni `Dev_Ops_New` yoluna göre revize edildi.
- **Discovery:** Yeni yapı için proje keşfi (`raios discover`) başlatıldı.
- **System Memory:** Proje hafızası yeni yapıya göre senkronize ediliyor.

<!-- MCP update by antigravity at 2026-05-06 03:01 -->
- [2026-05-06 03:01] **Implemented Git Diff Approval & Security Hardening (v0.8.0)**: Completed the full Git Diff Approval workflow. 
- Integrated safe_write with daemon-side RequestFileChange.
- Implemented TUI GitDiffView with syntax highlighting and human-in-the-loop approval (Y/N).
- Hardened agent execution proxy with process management and IPC sync.
- Resolved all compilation issues and naming collisions.
- Updated Cargo.toml for uuid serde support.

<!-- MCP update by antigravity at 2026-05-06 03:05 -->
- [2026-05-06 03:05] **Migrated Health Scanner to Daemon and enhanced State Sync workflow**: Successfully moved the periodic project health scanning from the TUI background threads to the aiosd daemon.
- Updated start_health_worker in src/daemon/health.rs to support broadcast notifications.
- Integrated StateSync broadcasting in the daemon whenever a health scan completes.
- Fixed borrowing and trait implementation issues (Clone/Debug for ProjectIndex).
- Verified that TUI correctly receives and applies full state updates via IPC.

<!-- MCP update by antigravity at 2026-05-06 03:06 -->
- [2026-05-06 03:06] **Implemented Background Git Worker for real-time project status monitoring**: Offloaded Git status tracking to the aiosd daemon.
- Created src/daemon/git.rs with start_git_worker to periodically scan all projects for branch names and dirty status.
- Integrated the Git worker into the daemon's broadcast system, ensuring TUI is notified of status changes without manual refreshes.
- This further solidifies the 'Thin Client / Powerful Daemon' architecture of R-AI-OS.

<!-- MCP update by antigravity at 2026-05-06 03:09 -->
- [2026-05-06 03:09] **Finalized R-AI-OS Daemon Architecture and GitHub Sync integration**: Completed all pending architectural tasks for R-AI-OS v0.8.0.
- Implemented GitHub Sync: GitHub stars and last update timestamps are now fetched in the background via `gh` CLI.
- Enhanced TUI Dashboard: Project details now include live GitHub stats.
- Automated entities.json Management: Discovered projects are now automatically saved to persistent storage by the daemon.
- Verified system stability with 0 compilation errors across daemon and TUI components.

<!-- MCP update by antigravity at 2026-05-06 03:10 -->
- [2026-05-06 03:10] **Created E2E Connection Test and Architecture Visualization**: Provided a Mermaid-based architecture diagram to visualize the R-AI-OS Client-Daemon communication flow.
- Created an E2E test script (Python) in the scratch directory to verify TCP/IPC connectivity and command/response cycles between raios and aiosd.
- Documented the connection protocol and health status in architecture_viz.md.

<!-- MCP update by antigravity at 2026-05-06 03:13 -->
- [2026-05-06 03:13] **Completed Gemini Task 1 and Fixed E2E Connection Issues**: Analyzed Core & Domain projects (Crucix, AI & Veri, Endüstriyel & Saha) and generated a comprehensive report in the brain directory.
- Fixed the WinError 10061 connection error by successfully starting the aiosd daemon background process.
- Verified TCP connectivity via the E2E test script (Handshake Successful).
- Mapped workspace root to C:\Users\turha\Desktop\Dev Ops and verified the presence of all core projects.

<!-- MCP update by antigravity at 2026-05-06 03:19 -->
- [2026-05-06 03:19] **Global installation of R-AI-OS v0.8.0 Aura Edition**: Updated Cargo.toml to version 0.8.0 and initiated a global installation via `cargo install`. 
- This enables the user to launch `raios` and `aiosd` from any terminal directory.
- Solidifies the project as a system-level utility tool.

<!-- MCP update by gemini at 2026-05-06 13:37 -->
- [2026-05-06 13:37] **R-AI-OS v0.8.0 Security & Async Workflow Hardening Completed**: R-AI-OS v0.8.0 sürümü başarıyla yayına alındı. 
1. **Güvenlik:** IPC katmanına Token-based Authentication (UUID handshake) eklendi. Yetkisiz erişimler engellendi.
2. **İş Akışı:** Eskiden kullanıcıyı kilitleyen blocking onay kutuları, asenkron 'Diff Inbox' kuyruk sistemiyle değiştirildi. Ajanlar arka planda çalışırken kullanıcı dashboard'da kalabilir, istediği zaman 'i' tuşuyla onay bekleyen değişiklikleri inceleyebilir.
3. **TUI:** Header'a canlı 'Inbox' sayacı eklendi. Diff view, birden fazla değişikliği yönetebilecek şekilde (Arrow keys navigation) refaktör edildi.
4. **E2E:** Hem güvenlik hem de asenkron iş akışı Python simülasyonları ile doğrulandı.
Sistem artık multi-agent operasyonlar için tam güvenli ve kesintisiz (non-blocking) bir altyapıya sahip.
