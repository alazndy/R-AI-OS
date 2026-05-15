# Faz 2: Sentinel Guard Watch — Design Spec
**Date:** 2026-05-15
**Project:** R-AI-OS
**Status:** Approved

---

## Problem

Dosya değişikliklerini gerçek zamanlı izleyerek API key sızıntılarını ve güvenlik açıklarını anında tespit eden bir mekanizma yok. Mevcut `scan_project()` manuel olarak çağrılmak zorunda.

## Goal

`raios security --watch` komutuyla aktif dizindeki dosyaları arka planda izlemek; değişiklik anında OWASP pattern taraması yapıp terminal + Windows toast bildirimi göstermek.

---

## Design

### CLI — `src/cli.rs`

```rust
Commands::Security {
    /// Directory to scan/watch (default: current dir)
    path: Option<PathBuf>,
    /// Watch mode — monitor file changes continuously
    #[arg(short, long)]
    watch: bool,
}
```

- `--watch` yok: `scan_project(path)` çalıştır, tek seferlik rapor bas
- `--watch` var: blocking notify loop, Ctrl+C ile dur

### security.rs Değişiklikleri

**Yeni sabit:**
```rust
pub const WATCHED_EXTS: &[&str] = &[
    "rs","ts","js","tsx","jsx","py","env","json","toml","yaml","yml"
];
```

**Yeni fonksiyon:**
```rust
pub fn scan_file(path: &Path) -> Vec<SecurityIssue>
```
`static_scan`'ın dosya düzeyi iç döngüsü çıkarılır. Tek dosyayı tarayarak `Vec<SecurityIssue>` döndürür.

### Watch Loop — `cmd_security_watch()`

```rust
let (tx, rx) = channel();
let mut watcher = notify::recommended_watcher(move |res| { let _ = tx.send(res); })?;
watcher.watch(path, RecursiveMode::Recursive)?;

loop {
    match rx.recv() {
        Ok(Ok(event)) if is_modify_or_create(&event) => {
            for file_path in event.paths.iter().filter(|p| is_watched_ext(p)) {
                let issues = scan_file(file_path);
                print_security_results(&issues, file_path, json);
                if !issues.is_empty() {
                    send_toast(&issues, file_path);
                }
            }
        }
        _ => {}
    }
}
```

Watcher, loop scope içinde tutulur — lifetime bug yoktur.

### Bağımlılık

`Cargo.toml`'a eklenecek:
```toml
notify-rust = "4"
```

### Çıktı Formatı

**Terminal (renkli stderr):**
```
⚠ CRITICAL [A02] Hardcoded API key — src/config.rs:14
   "api_key = \"sk-abc123...\""

✓  src/lib.rs clean (0 issues)
```

**JSON (`--json`):**
```json
[{"file":"src/config.rs","line":14,"owasp":"A02","severity":"CRITICAL","title":"Hardcoded API key","snippet":"api_key = ..."}]
```

**Windows Toast:**
```
Başlık: [RAIOS GUARD] CRITICAL
Gövde:  A02 · Hardcoded API key in config.rs:14
```

---

## Error Handling

| Durum | Davranış |
|-------|---------|
| Watcher init başarısız | `eprintln!` + `std::process::exit(1)` |
| Dosya okunamıyor | sessizce skip |
| Toast başarısız | sadece terminal çıktısı (non-fatal) |
| `--watch` yok, dizin yok | `eprintln!` + exit 1 |

---

## Tests (3 yeni unit test)

1. `scan_file` Critical pattern içeren temp dosyasında issue döndürüyor
2. `scan_file` temiz dosyada boş `Vec` döndürüyor
3. `WATCHED_EXTS` doğru uzantıları içeriyor

---

## Files Changed

| Dosya | Değişiklik |
|-------|-----------|
| `src/security.rs` | `pub const WATCHED_EXTS` + `pub fn scan_file()` |
| `src/cli.rs` | `Commands::Security` + `cmd_security()` + `cmd_security_watch()` |
| `Cargo.toml` | `notify-rust = "4"` |

---

## Out of Scope

- TUI notification panel entegrasyonu
- Faz 3 (Instinct Automation)
- semgrep watch modunda (sadece static pattern scan)
