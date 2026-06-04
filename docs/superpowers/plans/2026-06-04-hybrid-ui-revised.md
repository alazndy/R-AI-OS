# Hybrid UI — Revised Implementation Plan (Council Verdict 2026-06-04)

> **Council kararı:** Orijinal plandaki Tauri masaüstü uygulaması (Faz 2) iptal edildi.
> Önkoşul olarak server.rs refactor eklendi (Step 0).
> Faz 1 = HTTP adapter (rewrite değil). Faz 2 = VS Code sidebar (read-only önce).

**Hedef:** R-AI-OS daemon'ını VS Code extension'ın tüketebileceği HTTP/WebSocket API ile genişlet;
mevcut VS Code extension'a (v0.3.3) sidebar WebView Kanban ekle.

**Kapsam Dışı (iptal):** Tauri masaüstü uygulaması — CLI + TUI + VS Code extension yeterli.

**Tech Stack:** Rust (`axum` veya `warp`), TypeScript (VS Code API + WebView), SQLite WAL

---

## Step 0 — server.rs Dispatch Refactor (Önkoşul)

> Skeptic bulgusu: `src/mcp_server.rs` / `src/bin/raios.rs` içindeki string-match dispatch
> 948+ satır tek fonksiyon. Bu refactor olmadan yeni API yüzeyi eklemek unmaintainable.

### Yapılacaklar

- [ ] **Command handler'larını modüllere taşı:**
  Mevcut `match command_name { ... }` bloğunu aşağıdaki gibi ayrıştır:
  ```
  src/server/
    mod.rs          ← router + dispatch table
    handlers/
      build.rs
      security.rs
      git.rs
      version.rs
      health.rs
      swarm.rs
  ```
- [ ] **Router trait veya enum dispatch:** Her handler `async fn handle(&self, args: Value) -> Value` imzasını implement eder; `mod.rs` isim → handler map'i tutar.
- [ ] **Mevcut testler yeşil:** `cargo test` tüm 290 testi geçmeli, davranış değişmemeli.
- [ ] **Clippy temiz:** `cargo clippy -- -D warnings`

### Beklenen Çıktı
- `mcp_server.rs` ≤ 200 satır (router logic only)
- Her handler dosyası ≤ 150 satır
- `cargo test`: 290+ passed, 0 failed

### Commit
```
refactor: extract command handlers from mcp_server.rs into server/handlers/
```

---

## Faz 1 — HTTP/WebSocket API Adapter

> Mevcut TCP/JSON protokolü üstüne HTTP adapter — sıfırdan rewrite değil.
> Critic uyarısı: Bu yüzey security kernel'in en açık noktası — adversarial review şart.

### 1A — Güvenli Bootstrap Token

**Dosya:** `src/security/auth.rs` (yeni)

- [ ] Daemon başlarken kriptografik one-time `SessionToken` üret (`rand::thread_rng()`)
- [ ] Token'ı `~/.config/raios/.session_token` dosyasına yaz (chmod 600 — sadece owner okur)
- [ ] Token'ı `Authorization: Bearer <token>` header'ı ile her HTTP isteğinde doğrula
- [ ] Token 8 saat sonra otomatik expire — daemon restart ile yenilenir
- [ ] **Security Review Checklist:**
  - [ ] Token dosyası başka process tarafından okunabilir mi? (`stat` + permission check)
  - [ ] Timing-safe compare kullanıyor mu? (`constant_time_eq` crate)
  - [ ] Daemon port çakışmasında ne olur? (bind fail → clear error, no silent fallback)

**Testler:**
```rust
fn token_file_has_owner_only_permissions()
fn token_validation_rejects_wrong_token()
fn token_uses_constant_time_comparison()
```

### 1B — HTTP Endpoint'leri (axum)

**Dosya:** `src/server/http.rs` (yeni)

Mevcut TCP handler'larını HTTP'ye expose et — logic kopyalama yok, handler'ları çağır:

| Endpoint | Method | Handler | Açıklama |
|----------|--------|---------|----------|
| `/api/health` | GET | `handlers::health` | Sistem durumu |
| `/api/projects` | GET | `handlers::health::portfolio` | Workspace listesi |
| `/api/tasks` | GET | `handlers::swarm::list` | Todo/task listesi |
| `/api/approve` | POST | `handlers::swarm::approve` | Human-in-the-loop onay |
| `/api/stream` | WS | `radar::subscribe` | Canlı event stream |

**CORS:** Sadece `localhost` / `127.0.0.1` origin'lerine izin ver.
**Host header:** `localhost` veya `127.0.0.1` dışındaki istekleri 400 ile reddet (DNS rebinding koruması).

- [ ] `axum` veya `warp` Cargo.toml'a ekle
- [ ] HTTP server'ı daemon startup'ta TCP listener ile birlikte başlat (farklı port, default: `42070`)
- [ ] Port `42070` `raios-policy.toml`'dan override edilebilir
- [ ] `cargo test`: HTTP token auth testleri geçmeli

### 1C — SQLite WAL Modu

**Dosya:** `src/db.rs`

- [ ] `PRAGMA journal_mode=WAL;` connection açılışında çalıştır
- [ ] Eş zamanlı read/write kilitlenmelerini önler (VS Code + TUI aynı anda okuyabilir)

### Commit
```
feat: HTTP/WebSocket API adapter with bootstrap token auth (Faz 1)
```

---

## Faz 2 — VS Code Sidebar WebView

> Read-only Kanban önce. memory.md write-back ayrı görev olarak scope'landı.
> Critic bulgusu: drag-drop write-back = gizli schema migration — markdown round-trip parser bütçelenmeli.

### 2A — Webview Sidebar Panel (Read-Only)

**Dosya:** `vscode-extension/src/providers/SidebarProvider.ts` (yeni)

- [ ] `vscode.WebviewViewProvider` implement et
- [ ] Sidebar panel'i `package.json` `contributes.views` altına ekle (sol bar, her zaman görünür)
- [ ] WebView HTML/CSS: Geist Sans, dark mode, minimal glassmorphism (`backdrop-filter: blur(12px)`)
- [ ] Daemon'a HTTP GET `/api/projects` + `/api/tasks` çağrısı — token extension secret storage'dan alınır
- [ ] `vscode.window.registerWebviewViewProvider` ile kaydet

**Panel içeriği (read-only ilk aşama):**
```
┌─ R-AI-OS ──────────────────────────┐
│ 🟢 Daemon: connected               │
│                                    │
│ ACTIVE PROJECT                     │
│  R-AI-OS  Rust  v1.5.1             │
│  Build: ✓  Tests: 290/290          │
│                                    │
│ TASKS                              │
│  [ ] Step 0: server.rs refactor    │
│  [ ] Faz 1: HTTP adapter           │
│  [ ] Faz 2: VS Code sidebar        │
│                                    │
│ SECURITY                           │
│  Sandbox: ACTIVE                   │
│  Chain: ✓ verified                 │
└────────────────────────────────────┘
```

- [ ] Her 10 saniyede bir `/api/health` polling (WebSocket yoksa fallback)
- [ ] Daemon offline ise: "⚠️ Daemon not running — `raios daemon start`" mesajı

**Testler:**
- [ ] Token extension SecretStorage'dan doğru alınıyor mu?
- [ ] Daemon offline durumunda graceful fallback çalışıyor mu?

### 2B — memory.md Write-Back (Ayrı Görev — Sonraki Sprint)

> Bu scope'u şimdi implemente etme. Critic'in tespiti: markdown round-trip parser
> frontmatter + audit ledger referanslarını bozabilir. Önce okuma-yapısı stabil olsun.

Bir sonraki sprint için planlanan görevler:
- [ ] `memory.md` YAML frontmatter-aware parser yaz (existing libs: `gray-matter` / `remark`)
- [ ] Task satırı `- [ ]` → `- [x]` güncellemesi — sadece checkbox state, rest immutable
- [ ] Round-trip test: parse → serialize → diff (sadece checkbox değişmeli)
- [ ] Drag-drop reorder: task sırası değişince daemon'a PATCH `/api/tasks` çağrısı

### 2C — IPC Token Bridge

**Dosya:** `vscode-extension/src/ipc/TokenBridge.ts` (yeni)

> Skeptic uyarısı: WebView doğrudan disk okuyamaz. Extension host her çağrıyı broker etmeli.

```
WebView → postMessage → Extension Host → HTTP (Bearer token) → Daemon
```

- [ ] `vscode.ExtensionContext.secrets.store('raios.token', token)` ile token sakla
- [ ] Token'ı WebView'a asla gönderme — extension host proxy'si tüm API çağrılarını yapar
- [ ] `postMessage` mesaj tipleri: `{ type: 'fetch', endpoint: '/api/projects' }` → response mesajı
- [ ] Timeout: 5 saniye içinde cevap gelmezse WebView'a error state gönder

### Commit
```
feat: VS Code sidebar WebView with read-only dashboard (Faz 2)
```

---

## Faz 3 — Entegrasyon ve Testler

- [ ] **End-to-End:** Daemon başlat → VS Code sidebar bağlanıyor mu?
- [ ] **Token expiry:** 8 saat sonra sidebar graceful reconnect yapıyor mu?
- [ ] **Security smoke test:** Wrong token → 401, cross-origin request → 400
- [ ] **Fail-closed test:** Daemon durursa sidebar error state gösteriyor, crash yapmıyor
- [ ] **Regression:** `raios build .` hâlâ `"project_type": "Rust"` döndürüyor
- [ ] **Extension package:** `vsce package --no-dependencies` → `raios-0.4.0.vsix`

### Commit
```
chore: integration tests + raios-0.4.0.vsix packaging
```

---

## Öncelik Sırası

```
Step 0 (server.rs refactor)
    ↓
Faz 1A (bootstrap token)  →  Faz 1B (HTTP endpoints)  →  Faz 1C (WAL)
    ↓
Faz 2A (sidebar read-only)  →  Faz 2C (IPC bridge)
    ↓
Faz 3 (integration tests)
    ↓
[Sonraki Sprint] Faz 2B (memory.md write-back)
```

---

## İptal Edilen Kapsam

| Özellik | Neden İptal |
|---------|-------------|
| Tauri masaüstü uygulaması | CLI + TUI + VS Code yeterli; solo dev için ikinci ürün |
| System tray menü | Tauri ile birlikte iptal |
| Glassmorphism portfolio dashboard | Tauri ile birlikte iptal |
| Named Pipes / UDS mimarisi | TCP localhost yeterli; premature optimization |

---

## Başarı Kriterleri

- [ ] `cargo test`: 300+ passed, 0 failed
- [ ] `cargo clippy -- -D warnings`: 0 hata
- [ ] `raios audit https://example.com`: çalışıyor (regression yok)
- [ ] VS Code sidebar daemon'a bağlanıyor, task listesini gösteriyor
- [ ] Wrong token → HTTP 401 (security test)
- [ ] Daemon offline → sidebar graceful error (no crash)
