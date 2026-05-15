# Sentinel Guard Watch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `raios security [path] [--watch] [--json]` — tek seferlik OWASP taraması veya sürekli dosya izleme + terminal/toast bildirimi.

**Architecture:** `security.rs`'e `scan_file()` + `WATCHED_EXTS` eklenir. CLI'da yeni `Commands::Security` tanımlanır. Watch modu `notify` ile blocking loop çalıştırır, `notify-rust` ile Windows toast gönderir.

**Tech Stack:** Rust, `notify` (v6.1.1 — zaten mevcut), `notify-rust` (yeni), `regex_lite`, `clap`, `tempfile` (dev)

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `Cargo.toml` | `notify-rust = "4"` + `tempfile = "3"` (dev) |
| `src/security.rs` | `pub const WATCHED_EXTS` + `pub fn scan_file()` + 3 unit test |
| `src/cli.rs` | `Commands::Security` + match arm + `cmd_security()` + `cmd_security_watch()` + helpers |

---

## Task 1: Dependencies + `scan_file` — `Cargo.toml` + `src/security.rs`

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/security.rs`

- [ ] **Step 1.1: Cargo.toml'a bağımlılıkları ekle**

`[dependencies]` bloğuna:
```toml
notify-rust = "4"
```

`[dev-dependencies]` bloğuna (yoksa oluştur):
```toml
tempfile = "3"
```

- [ ] **Step 1.2: `cargo check` — build sağlam mı?**

```bash
cargo check 2>&1 | head -20
```

Beklenen: hata yok.

- [ ] **Step 1.3: Failing unit testleri yaz**

`src/security.rs` dosyasının en sonuna ekle:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn watched_exts_contains_expected() {
        assert!(WATCHED_EXTS.contains(&"rs"));
        assert!(WATCHED_EXTS.contains(&"env"));
        assert!(WATCHED_EXTS.contains(&"ts"));
        assert!(WATCHED_EXTS.contains(&"py"));
        assert!(!WATCHED_EXTS.contains(&"png"));
    }

    #[test]
    fn scan_file_detects_hardcoded_secret() {
        let mut f = tempfile::NamedTempFile::with_suffix(".env").unwrap();
        writeln!(f, r#"api_key = "sk-abc123456789abcdef""#).unwrap();
        let issues = scan_file(f.path());
        assert!(
            !issues.is_empty(),
            "Expected at least one issue for hardcoded api_key"
        );
        assert!(issues.iter().any(|i| i.owasp == "A02"));
    }

    #[test]
    fn scan_file_clean_file_returns_empty() {
        let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(f, "fn main() {{ println!(\"hello\"); }}").unwrap();
        let issues = scan_file(f.path());
        assert!(issues.is_empty(), "Expected no issues for clean file");
    }
}
```

- [ ] **Step 1.4: Testleri çalıştır — FAIL beklenir**

```bash
cargo test --lib security::tests -- --nocapture 2>&1 | head -15
```

Beklenen: `scan_file` ve `WATCHED_EXTS` bulunamadı hatası.

- [ ] **Step 1.5: `WATCHED_EXTS` sabitini ekle**

`src/security.rs`'teki `SKIP_DIRS` sabitinin hemen altına ekle:

```rust
/// File extensions monitored in `--watch` mode.
pub const WATCHED_EXTS: &[&str] = &[
    "rs", "ts", "js", "tsx", "jsx", "py", "env", "json", "toml", "yaml", "yml",
];
```

- [ ] **Step 1.6: `scan_file` fonksiyonunu ekle**

`src/security.rs`'teki `pub fn scan_project` fonksiyonunun hemen altına ekle:

```rust
/// Scan a single file for OWASP security patterns.
/// Returns empty Vec if the extension is not in WATCHED_EXTS or file cannot be read.
pub fn scan_file(path: &Path) -> Vec<SecurityIssue> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !WATCHED_EXTS.contains(&ext) {
        return vec![];
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let patterns_for_ext: Vec<&Pattern> =
        PATTERNS.iter().filter(|p| p.exts.contains(&ext)).collect();

    let mut issues = Vec::new();
    for pattern in &patterns_for_ext {
        let re = match regex_lite::Regex::new(pattern.pattern) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for (line_no, line) in content.lines().enumerate() {
            if re.is_match(line) {
                let snippet = line.trim().chars().take(80).collect::<String>();
                issues.push(SecurityIssue {
                    owasp: pattern.owasp,
                    title: pattern.title,
                    severity: pattern.severity.clone(),
                    file: Some(path.to_path_buf()),
                    line: Some(line_no + 1),
                    snippet: Some(snippet),
                });
                break;
            }
        }
    }
    issues
}
```

- [ ] **Step 1.7: Testleri çalıştır — PASS beklenir**

```bash
cargo test --lib security::tests -- --nocapture
```

Beklenen: `test result: ok. 3 passed`

- [ ] **Step 1.8: Commit**

```bash
git add Cargo.toml Cargo.lock src/security.rs
git commit -m "feat(security): add scan_file, WATCHED_EXTS, notify-rust dep"
```

---

## Task 2: CLI — `Commands::Security` + watch loop

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 2.1: `Commands::Security` varyantını ekle**

`src/cli.rs`'teki `Commands` enum'una ekle:

```rust
/// Scan for OWASP security issues, optionally watch for file changes
Security {
    /// Directory to scan/watch (default: current dir)
    path: Option<std::path::PathBuf>,
    /// Watch mode — monitor file changes continuously (Ctrl+C to stop)
    #[arg(short, long)]
    watch: bool,
},
```

- [ ] **Step 2.2: Match arm ekle**

`src/cli.rs`'teki `match cmd` bloğuna ekle:

```rust
Commands::Security { path, watch } => {
    cmd_security(path, watch, cli.json);
}
```

- [ ] **Step 2.3: `cmd_security` fonksiyonunu ekle**

```rust
fn cmd_security(path: Option<std::path::PathBuf>, watch: bool, json: bool) {
    let target = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if !target.exists() {
        eprintln!("Path does not exist: {}", target.display());
        std::process::exit(1);
    }

    if watch {
        if let Err(e) = cmd_security_watch(&target, json) {
            eprintln!("Guard error: {e}");
            std::process::exit(1);
        }
    } else {
        let report = crate::security::scan_project(&target);
        print_security_report(&report, json);
    }
}
```

- [ ] **Step 2.4: `print_security_report` fonksiyonunu ekle**

```rust
fn print_security_report(report: &crate::security::SecurityReport, json: bool) {
    if json {
        let issues: Vec<serde_json::Value> = report.issues.iter().map(|i| {
            serde_json::json!({
                "owasp": i.owasp,
                "severity": i.severity.label(),
                "title": i.title,
                "file": i.file.as_ref().map(|p| p.display().to_string()),
                "line": i.line,
                "snippet": i.snippet
            })
        }).collect();
        match serde_json::to_string_pretty(&issues) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
        return;
    }
    println!("Security scan: score={}/100 grade={}", report.score, report.grade);
    if report.issues.is_empty() {
        println!("✓ No issues found");
        return;
    }
    for issue in &report.issues {
        let file_info = issue.file.as_ref()
            .map(|p| format!(" — {}:{}", p.display(), issue.line.unwrap_or(0)))
            .unwrap_or_default();
        println!("⚠ {} [{}] {}{}", issue.severity.label(), issue.owasp, issue.title, file_info);
        if let Some(ref s) = issue.snippet {
            println!("   \"{}\"", s);
        }
    }
}
```

- [ ] **Step 2.5: `cmd_security_watch` fonksiyonunu ekle**

```rust
fn cmd_security_watch(path: &std::path::Path, json: bool) -> anyhow::Result<()> {
    use crate::security::{scan_file, WATCHED_EXTS};
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc::channel;

    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(path, RecursiveMode::Recursive)?;

    eprintln!("🛡  Guard watching {} (Ctrl+C to stop)", path.display());

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                use notify::EventKind;
                let is_relevant = matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                );
                if !is_relevant {
                    continue;
                }
                for file_path in &event.paths {
                    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if !WATCHED_EXTS.contains(&ext) {
                        continue;
                    }
                    let issues = scan_file(file_path);
                    print_guard_result(file_path, &issues, json);
                    if !issues.is_empty() {
                        send_guard_toast(file_path, &issues);
                    }
                }
            }
            Ok(Err(e)) => eprintln!("[guard] watcher error: {e}"),
            Err(_) => break,
        }
    }
    Ok(())
}
```

- [ ] **Step 2.6: `print_guard_result` fonksiyonunu ekle**

```rust
fn print_guard_result(
    path: &std::path::Path,
    issues: &[crate::security::SecurityIssue],
    json: bool,
) {
    if json {
        if issues.is_empty() {
            return;
        }
        let out: Vec<serde_json::Value> = issues.iter().map(|i| {
            serde_json::json!({
                "file": path.display().to_string(),
                "line": i.line,
                "owasp": i.owasp,
                "severity": i.severity.label(),
                "title": i.title,
                "snippet": i.snippet
            })
        }).collect();
        match serde_json::to_string_pretty(&out) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
        return;
    }
    if issues.is_empty() {
        eprintln!("✓  {} clean", path.display());
        return;
    }
    for issue in issues {
        eprintln!(
            "⚠ {} [{}] {} — {}:{}",
            issue.severity.label(),
            issue.owasp,
            issue.title,
            path.display(),
            issue.line.unwrap_or(0)
        );
        if let Some(ref s) = issue.snippet {
            eprintln!("   \"{}\"", s);
        }
    }
}
```

- [ ] **Step 2.7: `send_guard_toast` fonksiyonunu ekle**

```rust
fn send_guard_toast(path: &std::path::Path, issues: &[crate::security::SecurityIssue]) {
    let top = match issues.first() {
        Some(i) => i,
        None => return,
    };
    let filename = path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let body = format!(
        "{} · {} in {}:{}",
        top.owasp,
        top.title,
        filename,
        top.line.unwrap_or(0)
    );
    let summary = format!("[RAIOS GUARD] {}", top.severity.label());

    if let Err(e) = notify_rust::Notification::new()
        .summary(&summary)
        .body(&body)
        .show()
    {
        eprintln!("[guard] toast failed (non-fatal): {e}");
    }
}
```

- [ ] **Step 2.8: Build**

```bash
cargo build --bin raios 2>&1 | head -40
```

Beklenen: hata yok.

- [ ] **Step 2.9: Tüm testleri çalıştır**

```bash
cargo test --lib 2>&1 | tail -5
```

Beklenen: `test result: ok. 72 passed`

- [ ] **Step 2.10: Smoke test — tek seferlik tarama**

```bash
cargo run --bin raios -- security . 2>&1 | head -10
```

Beklenen: `Security scan: score=.../100 grade=...`

- [ ] **Step 2.11: Smoke test — JSON**

```bash
cargo run --bin raios -- --json security . 2>&1 | head -5
```

Beklenen: `[` ile başlayan geçerli JSON veya `[]`.

- [ ] **Step 2.12: Final commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): raios security --watch guard mode with toast notifications"
```
