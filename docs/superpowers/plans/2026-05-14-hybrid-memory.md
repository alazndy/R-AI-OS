# Hybrid Memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `raios memory --query "<text>" --top N` komutu tüm projelerin memory/AGENTS/MASTER/CLAUDE.md dosyalarında semantic arama yapabilsin.

**Architecture:** `Cortex`'e `search_with_filter()` + `index_memory_files()` metodları eklenir. CLI'da `Commands::Memory` enum'u `query` ve `top` argümanları alacak şekilde genişletilir. Cortex indeksi boşsa `--query` çağrılınca otomatik olarak sadece memory dosyaları indekslenir.

**Tech Stack:** Rust, `instant_distance` (HNSW), `fastembed`, `walkdir`, `clap`, `serde_json`

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/cortex/mod.rs` | `MEMORY_PATTERNS` sabiti (pub) + `index_memory_files()` + `search_with_filter()` + unit tests |
| `src/cli.rs` | `Commands::Memory` enum alanları + match arm + `cmd_memory()` + `cmd_memory_search()` |

`src/cortex/store.rs` değişmez — `VectorResult::score: f32` zaten satır 53'te mevcut.

---

## Task 1: `search_with_filter` + `index_memory_files` — `src/cortex/mod.rs`

**Files:**
- Modify: `src/cortex/mod.rs`

- [ ] **Step 1.1: Failing unit test yaz**

`src/cortex/mod.rs` dosyasının en sonuna (satır 228'den sonra) ekle:

```rust
#[cfg(test)]
mod tests {
    use crate::cortex::store::VectorResult;

    fn make_result(path: &str, score: f32) -> VectorResult {
        VectorResult { path: path.to_string(), start_line: 1, text: "x".into(), score }
    }

    fn filter_by_patterns(results: Vec<VectorResult>, patterns: &[&str]) -> Vec<VectorResult> {
        results
            .into_iter()
            .filter(|r| patterns.iter().any(|p| r.path.ends_with(p)))
            .collect()
    }

    #[test]
    fn filter_keeps_only_matching_files() {
        let results = vec![
            make_result("/proj/memory.md", 0.9),
            make_result("/proj/main.rs", 0.8),
            make_result("/proj/CLAUDE.md", 0.7),
            make_result("/proj/README.md", 0.6),
        ];
        let filtered = filter_by_patterns(results, &["memory.md", "CLAUDE.md"]);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|r| {
            r.path.ends_with("memory.md") || r.path.ends_with("CLAUDE.md")
        }));
    }

    #[test]
    fn filter_respects_top_k_limit() {
        let results = (0..20)
            .map(|i| make_result(&format!("/proj/{}/memory.md", i), 0.9 - i as f32 * 0.01))
            .collect::<Vec<_>>();
        let filtered: Vec<VectorResult> = filter_by_patterns(results, &["memory.md"])
            .into_iter()
            .take(5)
            .collect();
        assert_eq!(filtered.len(), 5);
    }

    #[test]
    fn filter_returns_empty_when_no_match() {
        let results = vec![make_result("/proj/main.rs", 0.9)];
        let filtered = filter_by_patterns(results, &["memory.md"]);
        assert!(filtered.is_empty());
    }
}
```

- [ ] **Step 1.2: Testleri çalıştır — PASS beklenir**

```bash
cargo test --lib cortex::tests -- --nocapture
```

Beklenen: `test result: ok. 3 passed`

- [ ] **Step 1.3: `MEMORY_PATTERNS` sabitini ekle**

`src/cortex/mod.rs` satır 29'daki `const INDEXED_EXTS` bloğunun hemen altına ekle:

```rust
pub const MEMORY_PATTERNS: &[&str] = &["memory.md", "AGENTS.md", "MASTER.md", "CLAUDE.md"];
```

- [ ] **Step 1.4: `index_memory_files` metodunu ekle**

`src/cortex/mod.rs`'teki `impl Cortex` bloğuna, mevcut `search()` metodundan (satır ~203) sonra ekle:

```rust
/// Index only memory/agents/master/CLAUDE files across the workspace.
/// Called automatically when the cortex store is empty and --query is used.
pub fn index_memory_files(&mut self, root: &Path) -> Result<usize> {
    let mut indexed = 0usize;

    let walker = WalkDir::new(root)
        .max_depth(8)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !SKIP_DIRS.contains(&n.as_ref())
                && !e.path().components().any(|c| c.as_os_str() == ".pnpm")
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());

    for entry in walker {
        let name = entry.file_name().to_string_lossy();
        if MEMORY_PATTERNS.iter().any(|p| name == *p) {
            if self.index_file(entry.path()).unwrap_or(false) {
                indexed += 1;
            }
        }
    }

    if indexed > 0 {
        self.engine.rebuild_hnsw();
        self.engine.save();
    }
    Ok(indexed)
}
```

- [ ] **Step 1.5: `search_with_filter` metodunu ekle**

`index_memory_files`'ın hemen altına ekle:

```rust
/// Semantic search filtered to files whose names match any of `filename_patterns`.
/// Pulls top_k * 10 candidates, filters by filename suffix, returns best top_k.
pub fn search_with_filter(
    &self,
    query: &str,
    top_k: usize,
    filename_patterns: &[&str],
) -> Result<Vec<VectorResult>> {
    let emb = self.embedder.embed_one(query)?;
    let candidates = self.engine.query(&emb, top_k * 10);
    let mut filtered: Vec<VectorResult> = candidates
        .into_iter()
        .filter(|r| filename_patterns.iter().any(|pat| r.path.ends_with(pat)))
        .take(top_k)
        .collect();
    filtered.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(filtered)
}
```

- [ ] **Step 1.6: Build kontrol**

```bash
cargo check 2>&1 | head -30
```

Beklenen: hata yok.

- [ ] **Step 1.7: Commit**

```bash
git add src/cortex/mod.rs
git commit -m "feat(cortex): add search_with_filter, index_memory_files, MEMORY_PATTERNS"
```

---

## Task 2: CLI genişletme — `src/cli.rs`

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 2.1: `Commands::Memory` enum varyantını güncelle**

`src/cli.rs` satır 32-35'teki `Memory` varyantını değiştir:

```rust
// ÖNCE:
/// Print project memory.md files
Memory {
    /// Project name filter
    project: Option<String>,
},

// SONRA:
/// Semantic search or print project memory.md files
Memory {
    /// Project name filter (omit to list all)
    project: Option<String>,
    /// Semantic search query across all memory/AGENTS/MASTER/CLAUDE files
    #[arg(short, long)]
    query: Option<String>,
    /// Number of results to show (default: 5)
    #[arg(short = 'n', long, default_value = "5")]
    top: usize,
},
```

- [ ] **Step 2.2: Match arm'ı güncelle**

`src/cli.rs` satır 313'teki match arm'ı değiştir:

```rust
// ÖNCE:
Commands::Memory { project } => cmd_memory(project, &cfg.dev_ops_path, cli.json),

// SONRA:
Commands::Memory { project, query, top } => {
    cmd_memory(project, query, top, &cfg.dev_ops_path, cli.json);
}
```

- [ ] **Step 2.3: `cmd_memory` fonksiyonunu güncelle**

`src/cli.rs` satır 459'daki `fn cmd_memory(project, dev_ops, json)` imzasını ve gövdesini tamamen değiştir:

```rust
fn cmd_memory(
    project: Option<String>,
    query: Option<String>,
    top: usize,
    dev_ops: &std::path::Path,
    json: bool,
) {
    if let Some(q) = query {
        cmd_memory_search(&q, top, dev_ops, json);
        return;
    }

    let files = discover_memory_files(dev_ops, 100);
    let mut results = Vec::new();

    if let Some(filter) = project {
        let f = filter.to_lowercase();
        for m in files {
            if m.name.to_lowercase().contains(&f) {
                if json {
                    results.push(m);
                } else {
                    println!("=== {} ===", m.name);
                    println!("{}", load_file_content(&m.path));
                }
                break;
            }
        }
    } else {
        for m in files.into_iter().take(5) {
            if json {
                results.push(m);
            } else {
                println!("=== {} ===", m.name);
                println!("{}", load_file_content(&m.path));
                println!();
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results).unwrap());
    }
}
```

- [ ] **Step 2.4: `cmd_memory_search` fonksiyonunu ekle**

`cmd_memory`'nin hemen altına yeni fonksiyon ekle:

```rust
fn cmd_memory_search(query: &str, top: usize, dev_ops: &std::path::Path, json: bool) {
    use crate::cortex::{Cortex, MEMORY_PATTERNS};

    let mut cortex = match Cortex::init() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cortex init failed: {e}. Falling back to plain listing.");
            return;
        }
    };

    if cortex.chunk_count() == 0 {
        eprintln!("Cortex index is empty — indexing memory files first…");
        match cortex.index_memory_files(dev_ops) {
            Ok(n) => eprintln!("Indexed {n} memory file(s)."),
            Err(e) => eprintln!("Index error: {e}"),
        }
    }

    let results = match cortex.search_with_filter(query, top, MEMORY_PATTERNS) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Search error: {e}");
            return;
        }
    };

    if results.is_empty() {
        eprintln!("No memory entries found for query. Try: raios cortex index");
        return;
    }

    if json {
        let json_out: Vec<serde_json::Value> = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let project = std::path::Path::new(&r.path)
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                serde_json::json!({
                    "rank": i + 1,
                    "score": r.score,
                    "project": project,
                    "file": r.path,
                    "line": r.start_line,
                    "snippet": r.text.trim()
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_out).unwrap());
    } else {
        for (i, r) in results.iter().enumerate() {
            let score_pct = (r.score * 100.0) as u32;
            let filename = std::path::Path::new(&r.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let project = std::path::Path::new(&r.path)
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let snippet = r.text
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>();
            println!("[{}] {}%  {} / {}:{}", i + 1, score_pct, project, filename, r.start_line);
            println!("    \"{}\"", snippet);
            println!();
        }
    }
}
```

- [ ] **Step 2.5: Build**

```bash
cargo build --bin raios 2>&1 | head -50
```

Beklenen: hata yok. Uyarı varsa `cargo clippy --fix -- -A clippy::all` ile temizle.

- [ ] **Step 2.6: Tüm testleri çalıştır**

```bash
cargo test 2>&1 | tail -10
```

Beklenen: `test result: ok. 69 passed` (66 mevcut + 3 yeni)

- [ ] **Step 2.7: Smoke test — query modu**

```bash
cargo run --bin raios -- memory --query "güvenlik" --top 3
```

Beklenen örnek çıktı:
```
Cortex index is empty — indexing memory files first…
Indexed 12 memory file(s).
[1] 84%  R-AI-OS / memory.md:45
    "Security scanning: semgrep + cargo audit"
```

- [ ] **Step 2.8: Smoke test — JSON modu**

```bash
cargo run --bin raios -- --json memory --query "sqlite" --top 2
```

Beklenen: geçerli JSON array `[{"rank":1,"score":...},...]`

- [ ] **Step 2.9: Eski davranış korunuyor mu?**

```bash
cargo run --bin raios -- memory
cargo run --bin raios -- memory R-AI-OS
```

Beklenen: önceki çıktıyla aynı.

- [ ] **Step 2.10: Final commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): raios memory --query semantic search with auto-index"
```
