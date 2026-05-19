# Plan 3: Edge-Intelligence — Local Fast-Path Routing

> **For agentic workers:** Use superpowers:subagent-driven-development to execute task-by-task.

**Goal:** `raios ask "3000 portunu kapat"` → yerel intent classifier → doğrudan CLI execute veya agent dispatch. LLM API'ye gitmeden basit komutlar anında çalışır.

**Architecture:** Phase 1: Rule-based classifier (regex/keyword). Phase 2: GGUF model (opsiyonel feature flag).

**Tech Stack:** Rust, mevcut `core/process.rs`, `core/git.rs`, `health.rs`, `tasks.rs`, `entities.rs`

**Mevcut durum:**
- `core/process::kill_port(port: u16)` var
- `core/process::list_ports()` var  
- `core/git::status(dir: &Path) -> Result<String>` var
- `health::check_project(proj)` var
- `tasks::dispatch_to_agent(task, agent, path, errors)` var

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/lib.rs` | `pub mod edge;` |
| `src/edge/mod.rs` | `Intent` enum, `pub use` |
| `src/edge/classifier.rs` | rule-based classify() + helpers |
| `src/edge/executor.rs` | execute(intent) → ExecutionResult |
| `src/cli.rs` | `Commands::Ask` + `cmd_ask()` |

---

## Task 1: `Intent` Enum + Module Scaffold

**Files:** `src/lib.rs`, `src/edge/mod.rs`

- [ ] **Step 1.1: `src/lib.rs`'e modül ekle**

Mevcut `pub mod` listesine (alfabetik sıraya göre):
```rust
pub mod edge;
```

- [ ] **Step 1.2: `src/edge/mod.rs` oluştur**

```rust
pub mod classifier;
pub mod executor;

pub use classifier::Intent;
pub use executor::{execute, ExecutionResult};
```

- [ ] **Step 1.3: `cargo check`**

```bash
cargo check 2>&1 | head -10
```

- [ ] **Step 1.4: Commit**

```bash
git add src/lib.rs src/edge/mod.rs
git commit -m "feat(edge): module scaffold"
```

---

## Task 2: Rule-Based Intent Classifier

**Files:** `src/edge/classifier.rs`

- [ ] **Step 2.1: Failing tests yaz**

`src/edge/classifier.rs` dosyası oluştur:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    PortKill(u16),
    ProcessList,
    FileFind { query: String },
    HealthCheck { project: Option<String> },
    GitStatus { project: Option<String> },
    Complex,
}

pub fn classify(input: &str) -> Intent {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_port_kill_turkish() {
        assert_eq!(classify("3000 portunu kapat"), Intent::PortKill(3000));
    }

    #[test]
    fn detects_port_kill_english() {
        assert_eq!(classify("kill port 8080"), Intent::PortKill(8080));
    }

    #[test]
    fn detects_process_list() {
        assert_eq!(classify("hangi portlar açık"), Intent::ProcessList);
        assert_eq!(classify("list open ports"), Intent::ProcessList);
    }

    #[test]
    fn detects_health_check() {
        assert_eq!(
            classify("R-AI-OS'un sağlığına bak"),
            Intent::HealthCheck { project: Some("R-AI-OS".to_string()) }
        );
    }

    #[test]
    fn detects_git_status() {
        assert_eq!(
            classify("git status göster"),
            Intent::GitStatus { project: None }
        );
    }

    #[test]
    fn complex_fallback() {
        assert_eq!(classify("auth bug'ını düzelt"), Intent::Complex);
    }
}
```

- [ ] **Step 2.2: Testleri çalıştır — FAIL beklenir**

```bash
cargo test --lib edge::classifier::tests -- --nocapture 2>&1 | head -10
```

- [ ] **Step 2.3: `classify()` implementasyonunu yaz**

`todo!()` yerine:

```rust
pub fn classify(input: &str) -> Intent {
    let lower = input.to_lowercase();

    // Port kill: "3000 kapat", "kill port 8080", "8080 durdur"
    let kill_keywords = ["kapat", "kill", "durdur", "close", "stop", "öldür"];
    if let Some(port) = extract_port_near_keyword(&lower, &kill_keywords) {
        return Intent::PortKill(port);
    }

    // Process list: "hangi portlar", "list ports", "açık portlar"
    let list_keywords = ["hangi port", "list port", "açık port", "open port",
                         "aktif port", "active port", "port listesi"];
    if list_keywords.iter().any(|k| lower.contains(k)) {
        return Intent::ProcessList;
    }

    // Health check: "sağlık", "health", "durum"
    let health_keywords = ["sağlık", "health check", "sağlığı", "durumu", "health"];
    if health_keywords.iter().any(|k| lower.contains(k)) {
        let project = extract_project_name(input);
        return Intent::HealthCheck { project };
    }

    // Git status
    let git_keywords = ["git status", "değişiklik", "degisiklik", "dirty", "uncommitted"];
    if git_keywords.iter().any(|k| lower.contains(k)) {
        let project = extract_project_name(input);
        return Intent::GitStatus { project };
    }

    // File find
    let find_keywords = ["bul", "ara ", "araştır", "find ", "search ", "where is"];
    if find_keywords.iter().any(|k| lower.contains(k)) {
        let query = extract_search_term(input);
        return Intent::FileFind { query };
    }

    Intent::Complex
}

fn extract_port_near_keyword(lower: &str, keywords: &[&str]) -> Option<u16> {
    let words: Vec<&str> = lower.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        // Number before keyword: "3000 kapat"
        if keywords.iter().any(|k| word.starts_with(k)) && i > 0 {
            if let Ok(port) = words[i - 1].parse::<u16>() {
                if port > 0 { return Some(port); }
            }
        }
        // Keyword before number: "kill port 8080"
        if keywords.iter().any(|k| word.starts_with(k)) && i + 1 < words.len() {
            // skip "port" word if present
            let next_idx = if words.get(i + 1).map(|w| *w == "port").unwrap_or(false) {
                i + 2
            } else {
                i + 1
            };
            if let Some(num_word) = words.get(next_idx) {
                if let Ok(port) = num_word.parse::<u16>() {
                    if port > 0 { return Some(port); }
                }
            }
        }
        // Number in word list near a kill keyword anywhere
        if let Ok(port) = word.parse::<u16>() {
            if port > 0 && keywords.iter().any(|k| lower.contains(k)) {
                return Some(port);
            }
        }
    }
    None
}

fn extract_project_name(input: &str) -> Option<String> {
    // Look for quoted project name: 'R-AI-OS', "project-name"
    for ch in ['\'', '"'] {
        if let Some(start) = input.find(ch) {
            if let Some(end) = input[start + 1..].find(ch) {
                let name = &input[start + 1..start + 1 + end];
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    // Look for "X'in", "X'un", "X'ın" patterns (Turkish genitive)
    let words: Vec<&str> = input.split_whitespace().collect();
    for word in &words {
        if word.contains("'i") || word.contains("'ı") || word.contains("'u") || word.contains("'ü") {
            let base = word.split('\'').next().unwrap_or("");
            if base.len() > 2 && base.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                return Some(base.to_string());
            }
        }
    }
    None
}

fn extract_search_term(input: &str) -> String {
    let lower = input.to_lowercase();
    let search_prefixes = ["bul ", "ara ", "araştır ", "find ", "search ", "where is "];
    for prefix in &search_prefixes {
        if let Some(pos) = lower.find(prefix) {
            let after = input[pos + prefix.len()..].trim();
            if !after.is_empty() {
                return after.split_whitespace().next().unwrap_or(after).to_string();
            }
        }
    }
    input.split_whitespace().last().unwrap_or(input).to_string()
}
```

- [ ] **Step 2.4: Testleri çalıştır — PASS beklenir**

```bash
cargo test --lib edge::classifier::tests -- --nocapture
```

Beklenen: `test result: ok. 6 passed`

- [ ] **Step 2.5: Commit**

```bash
git add src/edge/classifier.rs
git commit -m "feat(edge): rule-based intent classifier — Turkish/English support, 6 tests"
```

---

## Task 3: Intent Executor

**Files:** `src/edge/executor.rs`

- [ ] **Step 3.1: `src/edge/executor.rs` oluştur**

```rust
use crate::edge::classifier::Intent;
use anyhow::Result;
use std::path::Path;

#[derive(Debug)]
pub struct ExecutionResult {
    pub output: String,
    pub success: bool,
}

/// Execute a classified intent. Returns DISPATCH_TO_AGENT for Complex intents.
pub fn execute(intent: Intent, dev_ops: &Path) -> Result<ExecutionResult> {
    match intent {
        Intent::PortKill(port) => {
            match crate::core::process::kill_port(port) {
                Ok(true) => Ok(ExecutionResult {
                    output: format!("✓ Port {} kapatıldı", port),
                    success: true,
                }),
                Ok(false) => Ok(ExecutionResult {
                    output: format!("Port {} zaten kapalı veya bulunamadı", port),
                    success: true,
                }),
                Err(e) => Ok(ExecutionResult {
                    output: format!("Port {} kapatılamadı: {}", port, e),
                    success: false,
                }),
            }
        }

        Intent::ProcessList => {
            match crate::core::process::list_ports() {
                Ok(ports) if ports.is_empty() => Ok(ExecutionResult {
                    output: "Açık port bulunamadı".into(),
                    success: true,
                }),
                Ok(ports) => {
                    let lines: Vec<String> = ports
                        .iter()
                        .map(|p| format!("  :{:<5} {} (PID {})", p.port, p.name, p.pid))
                        .collect();
                    Ok(ExecutionResult {
                        output: format!("Açık portlar ({}):\n{}", ports.len(), lines.join("\n")),
                        success: true,
                    })
                }
                Err(e) => Ok(ExecutionResult {
                    output: format!("Port listesi alınamadı: {}", e),
                    success: false,
                }),
            }
        }

        Intent::HealthCheck { project } => {
            let projects = crate::entities::load_entities(dev_ops);
            let proj = if let Some(ref name) = project {
                let n = name.to_lowercase();
                projects.into_iter().find(|p| p.name.to_lowercase().contains(&n))
            } else {
                let cwd = std::env::current_dir().unwrap_or_default();
                projects.into_iter().find(|p| p.local_path == cwd)
            };

            match proj {
                Some(p) => {
                    let h = crate::health::check_project(&p);
                    let git = match h.git_dirty {
                        Some(true) => "dirty", Some(false) => "clean", None => "?",
                    };
                    Ok(ExecutionResult {
                        output: format!(
                            "{}: {}/100 ({}) | Security:{} | Refactor:{} | Git:{}",
                            h.name,
                            h.compliance_score.unwrap_or(0),
                            h.compliance_grade,
                            h.security_grade.as_deref().unwrap_or("-"),
                            h.refactor_grade,
                            git,
                        ),
                        success: true,
                    })
                }
                None => Ok(ExecutionResult {
                    output: format!(
                        "Proje bulunamadı{}. Try: raios ask \"<proje adı> health\"",
                        project.map(|p| format!(": {}", p)).unwrap_or_default()
                    ),
                    success: false,
                }),
            }
        }

        Intent::GitStatus { project } => {
            let path = if let Some(ref name) = project {
                let projects = crate::entities::load_entities(dev_ops);
                let n = name.to_lowercase();
                projects
                    .into_iter()
                    .find(|p| p.name.to_lowercase().contains(&n))
                    .map(|p| p.local_path)
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            } else {
                std::env::current_dir().unwrap_or_default()
            };

            match crate::core::git::status(&path) {
                Ok(s) if s.trim().is_empty() => Ok(ExecutionResult {
                    output: "✓ Working tree clean".into(),
                    success: true,
                }),
                Ok(s) => Ok(ExecutionResult { output: s, success: true }),
                Err(e) => Ok(ExecutionResult {
                    output: format!("git status failed: {}", e),
                    success: false,
                }),
            }
        }

        Intent::FileFind { query } => {
            // Delegate to raios search
            Ok(ExecutionResult {
                output: format!(
                    "Aranıyor: '{}'\nHint: raios search \"{}\" komutu ile daha kapsamlı arama yapabilirsin.",
                    query, query
                ),
                success: true,
            })
        }

        Intent::Complex => Ok(ExecutionResult {
            output: "DISPATCH_TO_AGENT".into(),
            success: false,
        }),
    }
}
```

- [ ] **Step 3.2: `cargo check`**

```bash
cargo check 2>&1 | head -20
```

- [ ] **Step 3.3: Commit**

```bash
git add src/edge/executor.rs
git commit -m "feat(edge): intent executor — port kill, process list, health, git status"
```

---

## Task 4: `raios ask` CLI Komutu

**Files:** `src/cli.rs`

- [ ] **Step 4.1: `Commands::Ask` varyantı ekle**

Mevcut son varyanttan sonra:

```rust
/// Natural language command routing (Edge-Intelligence)
/// Examples: "3000 portunu kapat", "R-AI-OS health", "git status göster"
Ask {
    /// Natural language query in Turkish or English
    query: String,
    /// Force dispatch to agent even for simple intents
    #[arg(long)]
    force_agent: bool,
    /// Show detected intent without executing
    #[arg(long)]
    dry_run: bool,
},
```

- [ ] **Step 4.2: Match arm ekle**

```rust
Commands::Ask { query, force_agent, dry_run } => {
    cmd_ask(&query, force_agent, dry_run, &cfg.dev_ops_path, cli.json);
}
```

- [ ] **Step 4.3: `cmd_ask()` fonksiyonu ekle**

```rust
fn cmd_ask(query: &str, force_agent: bool, dry_run: bool, dev_ops: &std::path::Path, json: bool) {
    use crate::edge::{classifier, executor, Intent};

    let intent = if force_agent {
        Intent::Complex
    } else {
        classifier::classify(query)
    };

    if dry_run {
        println!("Query:  {}", query);
        println!("Intent: {:?}", intent);
        println!("(Dry run — use without --dry-run to execute)");
        return;
    }

    if !json {
        println!("→ {:?}", intent);
    }

    match executor::execute(intent.clone(), dev_ops) {
        Ok(result) if result.success => {
            if json {
                println!("{}", serde_json::json!({"output": result.output, "success": true}));
            } else {
                println!("{}", result.output);
            }
        }
        Ok(_) if matches!(intent, Intent::Complex) => {
            // Dispatch to task router
            if !json { println!("Ajan'a yönlendiriliyor..."); }
            let task = crate::tasks::Task {
                text: query.to_string(),
                completed: false,
                agent: Some("claude".to_string()),
                project: None,
            };
            let result = crate::tasks::dispatch_to_agent(&task, "claude", None, None);
            if json {
                println!("{}", serde_json::json!({"dispatched": true, "result": result}));
            } else {
                println!("{}", result);
            }
        }
        Ok(result) => {
            if json {
                println!("{}", serde_json::json!({"output": result.output, "success": false}));
            } else {
                eprintln!("⚠ {}", result.output);
            }
        }
        Err(e) => eprintln!("Hata: {e}"),
    }
}
```

- [ ] **Step 4.4: `cargo build --bin raios`**

```bash
cargo build --bin raios 2>&1 | tail -5
```

- [ ] **Step 4.5: Smoke testler**

```bash
cargo run --bin raios -- ask "hangi portlar açık" --dry-run
```

Beklenen çıktı:
```
Query:  hangi portlar açık
Intent: ProcessList
(Dry run — use without --dry-run to execute)
```

```bash
cargo run --bin raios -- ask "R-AI-OS health" --dry-run
```

Beklenen:
```
Query:  R-AI-OS health
Intent: HealthCheck { project: Some("R-AI-OS") }
```

```bash
cargo run --bin raios -- ask "auth bug'ını düzelt" --dry-run
```

Beklenen:
```
Query:  auth bug'ını düzelt
Intent: Complex
```

- [ ] **Step 4.6: Tüm testleri çalıştır**

```bash
cargo test --lib 2>&1 | grep "test result"
```

- [ ] **Step 4.7: Final commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): raios ask — edge-intelligence NL routing with dry-run support"
```
