# Plan 3: Edge-Intelligence — Local Fast-Path Routing

> **Subagent-Driven Development** ile execute et. Cactus-Compute/Needle'dan ilham alındı.

**Goal:** `aiosd` içine küçük bir local model entegre ederek basit sistem komutları (port kill, file search, health check) için LLM API'ye gitmeden anında cevap ver.

**Philosophy:** "Düşün → Yönlendir → Çalıştır" — Basit niyetler yerel, karmaşık niyetler Claude/Gemini'ye.

**Architecture:** `raios ask "3000 portunu kapat"` → `EdgeRouter` → intent classification → local executekuyruk (basit) veya agent dispatch (karmaşık).

**Tech Stack:** Rust, `llama-cpp-2` crate (GGUF model) veya `candle` + küçük model (Phi-3 mini / TinyLlama), mevcut CLI komut sistemi.

---

## Model Seçimi

| Model | Boyut | Hız | Yeterlilik |
|-------|-------|-----|-----------|
| Phi-3 mini (Q4) | ~2GB | ~50 tok/s CPU | Komut routing için yeterli |
| TinyLlama 1.1B (Q4) | ~700MB | ~80 tok/s CPU | Daha hızlı, daha az akıllı |
| Gemma 2B (Q4) | ~1.5GB | ~60 tok/s CPU | İyi denge |

**Tavsiye:** TinyLlama 1.1B başlangıç için — küçük, hızlı, Rust `llama-cpp-2` ile çalışır.

---

## Intent Taxonomy

```
SIMPLE (local execute):
  - port_kill: "3000 portunu kapat"
  - file_find: "src'de AuthError araştır"
  - health_check: "R-AI-OS'un sağlığına bak"
  - git_status: "değişiklikleri göster"
  - process_list: "hangi portlar açık"

COMPLEX (agent dispatch):
  - code_change: "auth bug'ını düzelt"
  - feature_request: "dark mode ekle"
  - refactor: "services.rs'i temizle"
  - unknown: fallback to Claude
```

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/edge/mod.rs` | Yeni — `EdgeRouter` |
| `src/edge/classifier.rs` | Yeni — intent classification |
| `src/edge/executor.rs` | Yeni — simple intent executor |
| `src/edge/model.rs` | Yeni — GGUF model loader |
| `src/cli.rs` | `Commands::Ask` |
| `src/lib.rs` | `pub mod edge;` |
| `Cargo.toml` | `llama-cpp-2` veya `candle-core` |

---

## Task 1: Intent Classifier (rule-based MVP)

**Strategy:** Model entegrasyonundan önce rule-based classifier ile başla. Model çalışmıyorsa graceful fallback.

**Files:** `src/edge/classifier.rs`

- [ ] Oluştur:
```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    PortKill(u16),
    FileFind { query: String },
    HealthCheck { project: Option<String> },
    GitStatus { project: Option<String> },
    ProcessList,
    Complex,
}

pub fn classify(input: &str) -> Intent {
    let lower = input.to_lowercase();

    // Port kill patterns
    if let Some(port) = extract_port(&lower, &["kapat", "kill", "close", "stop"]) {
        return Intent::PortKill(port);
    }

    // File search
    if lower.contains("araştır") || lower.contains("bul") || lower.contains("find") || lower.contains("search") {
        let query = extract_search_term(input);
        return Intent::FileFind { query };
    }

    // Health
    if lower.contains("sağlık") || lower.contains("health") || lower.contains("durum") {
        let project = extract_project_name(input);
        return Intent::HealthCheck { project };
    }

    // Git
    if lower.contains("değişiklik") || lower.contains("status") || lower.contains("git") {
        let project = extract_project_name(input);
        return Intent::GitStatus { project };
    }

    // Process list
    if lower.contains("port") && (lower.contains("liste") || lower.contains("açık") || lower.contains("open")) {
        return Intent::ProcessList;
    }

    Intent::Complex
}

fn extract_port(s: &str, keywords: &[&str]) -> Option<u16> {
    let words: Vec<&str> = s.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        if keywords.iter().any(|k| word.contains(k)) {
            // Look for number near keyword
            for j in i.saturating_sub(2)..=(i + 2).min(words.len() - 1) {
                if let Ok(port) = words[j].parse::<u16>() {
                    return Some(port);
                }
            }
        }
    }
    // Also: number before keyword
    for window in words.windows(2) {
        if let Ok(port) = window[0].parse::<u16>() {
            if keywords.iter().any(|k| window[1].contains(k)) {
                return Some(port);
            }
        }
    }
    None
}

fn extract_search_term(input: &str) -> String {
    // Return words after search keywords
    let lower = input.to_lowercase();
    for kw in ["araştır", "bul", "find", "search", "için"] {
        if let Some(pos) = lower.find(kw) {
            let before = input[..pos].trim().split_whitespace().last().unwrap_or("").to_string();
            if !before.is_empty() {
                return before;
            }
            let after = input[pos + kw.len()..].trim().to_string();
            if !after.is_empty() {
                return after.split_whitespace().next().unwrap_or("").to_string();
            }
        }
    }
    input.split_whitespace().last().unwrap_or(input).to_string()
}

fn extract_project_name(_input: &str) -> Option<String> {
    // Future: NER — for now return None (use current dir)
    None
}
```

- [ ] Unit tests: 10 test case (Türkçe + İngilizce patterns)

- [ ] Commit: `feat(edge): rule-based intent classifier with Turkish/English support`

---

## Task 2: Simple Intent Executor

**Files:** `src/edge/executor.rs`

- [ ] Oluştur:
```rust
use crate::edge::classifier::Intent;
use anyhow::Result;

pub struct ExecutionResult {
    pub output: String,
    pub success: bool,
}

pub fn execute(intent: Intent, dev_ops: &std::path::Path) -> Result<ExecutionResult> {
    match intent {
        Intent::PortKill(port) => {
            let result = crate::core::process::kill_port(port)?;
            Ok(ExecutionResult {
                output: if result { format!("✓ Port {} kapatıldı", port) }
                        else { format!("Port {} zaten kapalı veya bulunamadı", port) },
                success: true,
            })
        }
        Intent::ProcessList => {
            let ports = crate::core::process::list_ports()?;
            let output = ports.iter()
                .map(|p| format!("  :{} — {} ({})", p.port, p.name, p.pid))
                .collect::<Vec<_>>().join("\n");
            Ok(ExecutionResult { output, success: true })
        }
        Intent::HealthCheck { project } => {
            let projects = crate::entities::load_entities(dev_ops);
            let proj = if let Some(name) = project {
                projects.into_iter().find(|p| p.name.to_lowercase().contains(&name.to_lowercase()))
            } else {
                let cwd = std::env::current_dir().unwrap_or_default();
                projects.into_iter().find(|p| p.local_path == cwd)
            };
            match proj {
                Some(p) => {
                    let h = crate::health::check_project(&p);
                    Ok(ExecutionResult {
                        output: format!("{}: {}/100 ({}) | Security: {} | Refactor: {}",
                            h.name, h.compliance_score.unwrap_or(0),
                            h.compliance_grade, h.security_grade.as_deref().unwrap_or("-"),
                            h.refactor_grade),
                        success: true,
                    })
                }
                None => Ok(ExecutionResult {
                    output: "Proje bulunamadı".into(),
                    success: false,
                })
            }
        }
        Intent::GitStatus { project } => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let path = project.map(|_| cwd.clone()).unwrap_or(cwd);
            let status = crate::core::git::status(&path)?;
            Ok(ExecutionResult { output: status, success: true })
        }
        Intent::FileFind { query } => {
            let cwd = std::env::current_dir().unwrap_or_default();
            // Use existing cortex search
            let output = format!("raios search \"{}\" komutu çalıştırılıyor...", query);
            Ok(ExecutionResult { output, success: true })
        }
        Intent::Complex => Ok(ExecutionResult {
            output: "DISPATCH_TO_AGENT".into(),
            success: false,
        })
    }
}
```

- [ ] `cargo check` → temiz

- [ ] Commit: `feat(edge): simple intent executor — port kill, health, git, process list`

---

## Task 3: `raios ask` CLI komutu

**Files:** `src/cli.rs`

- [ ] `Commands::Ask` ekle:
```rust
/// Natural language command routing (Edge-Intelligence)
Ask {
    /// Natural language query (Turkish or English)
    query: String,
    /// Force agent dispatch even for simple intents
    #[arg(long)]
    force_agent: bool,
},
```

- [ ] `cmd_ask()`:
```rust
fn cmd_ask(query: &str, force_agent: bool, dev_ops: &Path, json: bool) {
    use crate::edge::{classifier, executor};

    let intent = if force_agent {
        classifier::Intent::Complex
    } else {
        classifier::classify(query)
    };

    println!("Intent: {:?}", intent);

    let result = executor::execute(intent.clone(), dev_ops);
    match result {
        Ok(r) if r.success => println!("{}", r.output),
        Ok(_) => {
            // Complex — dispatch to task router
            println!("Ajan'a yönlendiriliyor: {}", query);
            cmd_task(Some(query.to_string()), None, dev_ops, json);
        }
        Err(e) => eprintln!("Hata: {e}"),
    }
}
```

- [ ] Smoke test: `raios ask "3000 portunu kapat"` → port kill

- [ ] Commit: `feat(cli): raios ask — edge-intelligence natural language routing`

---

## Task 4: GGUF Model Integration (Phase 2 — Opsiyonel)

**Prerequisite:** Rule-based sistem production'da stabil olduktan sonra.

- [ ] `Cargo.toml`'a: `llama-cpp-2 = { version = "0.1", optional = true }` (feature flag: `edge-llm`)

- [ ] `src/edge/model.rs` — model loader + inference:
  - Model path: `~/.raios/models/tinyllama.gguf`
  - Download on first use (progress bar)
  - Inference timeout: 3s (fallback to rule-based if exceeded)

- [ ] `classifier.rs`'de model-based sınıflandırma:
  - Prompt: `"Classify this command: {input}\nCategories: port_kill|file_find|health_check|git_status|process_list|complex\nAnswer:"`
  - Parse ilk token → intent mapping

- [ ] Feature flag ile aktive et: `cargo build --features edge-llm`

- [ ] Commit: `feat(edge): GGUF model integration for LLM-based intent classification`
