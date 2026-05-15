# Instinct Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** `raios instinct add/list/suggest` — manuel + otomatik instinct yönetimi; global JSON + per-project memory.md kaydı.

**Architecture:** `src/instinct.rs`'e 3 yeni public fonksiyon. CLI'da `Commands::Instinct` + 3 alt komut. `raios health` çıktısına öneri footer'ı.

**Tech Stack:** Rust, `anyhow`, `serde_json`, `std::io`, mevcut `InstinctEngine` + `ProjectHealth`

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/instinct.rs` | `suggest_from_health` + `append_to_memory_md` + `load_project_rules` + 3 test |
| `src/cli.rs` | `Commands::Instinct` + `InstinctCmd` + `cmd_instinct_*` + health footer |

---

## Task 1: `instinct.rs` — 3 yeni fonksiyon + testler

**Files:** Modify `src/instinct.rs`

- [ ] **Step 1.1: Failing unit testleri yaz**

`src/instinct.rs` sonuna ekle:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::ProjectHealth;
    use tempfile::TempDir;

    fn make_bad_health(tmp: &TempDir) -> ProjectHealth {
        ProjectHealth {
            name: "test-proj".into(),
            path: tmp.path().to_path_buf(),
            status: "active".into(),
            git_dirty: Some(true),
            remote_url: None,
            compliance_score: Some(40),
            compliance_grade: "D".into(),
            has_memory: false,
            has_sigmap: false,
            constitution_issues: vec!["pnpm".into(), "rls".into(), "api_key".into()],
            graphify_done: false,
            graph_report: None,
            security_score: Some(50),
            security_grade: Some("C".into()),
            security_issue_count: 3,
            security_critical: 2,
            refactor_score: 40,
            refactor_grade: "F".into(),
            refactor_high_count: 5,
            refactor_medium_count: 3,
        }
    }

    #[test]
    fn suggest_from_health_returns_suggestions_for_bad_project() {
        let tmp = TempDir::new().unwrap();
        let health = make_bad_health(&tmp);
        let suggestions = suggest_from_health(&health);
        assert!(!suggestions.is_empty(), "Expected suggestions for bad project");
        assert!(suggestions.iter().any(|s| s.contains("Refactor")));
    }

    #[test]
    fn append_to_memory_md_creates_instincts_section() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("memory.md"), "# Project Memory\n\n## Notes\n- note\n").unwrap();
        append_to_memory_md(tmp.path(), "Never use malloc here").unwrap();
        let content = std::fs::read_to_string(tmp.path().join("memory.md")).unwrap();
        assert!(content.contains("## Instincts"));
        assert!(content.contains("Never use malloc here"));
    }

    #[test]
    fn append_to_memory_md_does_not_duplicate() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("memory.md"), "# Memory\n").unwrap();
        append_to_memory_md(tmp.path(), "No duplicates rule").unwrap();
        append_to_memory_md(tmp.path(), "No duplicates rule").unwrap();
        let content = std::fs::read_to_string(tmp.path().join("memory.md")).unwrap();
        assert_eq!(content.matches("No duplicates rule").count(), 1);
    }
}
```

- [ ] **Step 1.2: Testleri çalıştır — FAIL beklenir**

```bash
cargo test --lib instinct::tests -- --nocapture 2>&1 | head -15
```

- [ ] **Step 1.3: `suggest_from_health` fonksiyonunu ekle**

`src/instinct.rs`'e (`InstinctEngine` impl bloğunun dışına) ekle:

```rust
use crate::health::ProjectHealth;

pub fn suggest_from_health(health: &ProjectHealth) -> Vec<String> {
    let mut suggestions = Vec::new();

    if matches!(health.refactor_grade.as_str(), "D" | "F") {
        suggestions.push(format!(
            "Refactor grade {} — high nesting/unwrap chains, clean before new features",
            health.refactor_grade
        ));
    }
    if health.security_critical > 0 {
        suggestions.push(format!(
            "Has {} CRITICAL security issue(s) — run `raios security` before commit",
            health.security_critical
        ));
    }
    if !health.has_memory {
        suggestions.push("No memory.md — add one to track decisions and learnings".into());
    }
    if !health.has_sigmap {
        suggestions.push("No SIGMAP.md — run sigmap to generate context map".into());
    }
    if health.git_dirty == Some(true) {
        suggestions.push("Uncommitted changes detected — commit before context switch".into());
    }
    if health.constitution_issues.len() > 2 {
        suggestions.push(format!(
            "Multiple constitution violations ({}) — review MASTER.md",
            health.constitution_issues.len()
        ));
    }
    suggestions
}
```

- [ ] **Step 1.4: `append_to_memory_md` fonksiyonunu ekle**

```rust
pub fn append_to_memory_md(project_path: &std::path::Path, rule: &str) -> anyhow::Result<()> {
    let memory_path = project_path.join("memory.md");
    if !memory_path.exists() {
        anyhow::bail!("memory.md not found at {}", memory_path.display());
    }

    let content = std::fs::read_to_string(&memory_path)?;
    if content.contains(rule) {
        return Ok(()); // duplicate
    }

    let new_content = if content.contains("## Instincts") {
        let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
        let insert_pos = lines
            .iter()
            .position(|l| l.trim() == "## Instincts")
            .map(|p| p + 1)
            .unwrap_or(lines.len());
        lines.insert(insert_pos, format!("- {}", rule));
        let joined = lines.join("\n");
        if content.ends_with('\n') { format!("{}\n", joined) } else { joined }
    } else {
        format!("{}\n## Instincts\n- {}\n", content.trim_end_matches('\n'), rule)
    };

    std::fs::write(&memory_path, new_content)?;
    Ok(())
}
```

- [ ] **Step 1.5: `load_project_rules` fonksiyonunu ekle**

```rust
pub fn load_project_rules(project_path: &std::path::Path) -> Vec<String> {
    let memory_path = project_path.join("memory.md");
    let content = match std::fs::read_to_string(&memory_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut in_section = false;
    let mut rules = Vec::new();
    for line in content.lines() {
        if line.trim() == "## Instincts" {
            in_section = true;
            continue;
        }
        if in_section {
            if line.starts_with("## ") { break; }
            if let Some(rule) = line.trim().strip_prefix("- ") {
                rules.push(rule.to_string());
            }
        }
    }
    rules
}
```

- [ ] **Step 1.6: Testleri çalıştır — PASS beklenir**

```bash
cargo test --lib instinct::tests -- --nocapture
```

Beklenen: `test result: ok. 3 passed`

- [ ] **Step 1.7: Commit**

```bash
git add src/instinct.rs
git commit -m "feat(instinct): suggest_from_health, append_to_memory_md, load_project_rules"
```

---

## Task 2: CLI — `Commands::Instinct` + health footer

**Files:** Modify `src/cli.rs`

- [ ] **Step 2.1: `InstinctCmd` sub-enum ekle**

`src/cli.rs`'e `Commands` enum'undan önce ekle:

```rust
#[derive(Subcommand)]
pub enum InstinctCmd {
    /// Add a rule manually to global instincts + project memory.md
    Add {
        /// The rule text
        rule: String,
        /// Project path (default: current dir)
        #[arg(short, long)]
        path: Option<std::path::PathBuf>,
    },
    /// List all instincts (global + current project)
    List {
        /// Project path (default: current dir)
        #[arg(short, long)]
        path: Option<std::path::PathBuf>,
    },
    /// Suggest instincts from health analysis with interactive approval
    Suggest {
        /// Project name or path
        project: Option<String>,
    },
}
```

- [ ] **Step 2.2: `Commands::Instinct` varyantını ekle**

`Commands` enum'una ekle:

```rust
/// Manage project instincts (learned rules)
Instinct {
    #[command(subcommand)]
    cmd: InstinctCmd,
},
```

- [ ] **Step 2.3: Match arm ekle**

`match cmd` bloğuna ekle:

```rust
Commands::Instinct { cmd } => {
    cmd_instinct(cmd, &cfg.dev_ops_path, cli.json);
}
```

- [ ] **Step 2.4: `cmd_instinct` router ekle**

```rust
fn cmd_instinct(cmd: InstinctCmd, dev_ops: &std::path::Path, json: bool) {
    match cmd {
        InstinctCmd::Add { rule, path } => cmd_instinct_add(&rule, path, json),
        InstinctCmd::List { path } => cmd_instinct_list(path, json),
        InstinctCmd::Suggest { project } => cmd_instinct_suggest(project, dev_ops, json),
    }
}
```

- [ ] **Step 2.5: `cmd_instinct_add` ekle**

```rust
fn cmd_instinct_add(rule: &str, path: Option<std::path::PathBuf>, json: bool) {
    use crate::instinct::{append_to_memory_md, InstinctEngine};

    let mut engine = InstinctEngine::init();
    engine.add_rule(rule.to_string());
    if let Err(e) = engine.save() {
        eprintln!("Failed to save instinct: {e}");
        return;
    }

    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let memory_ok = match append_to_memory_md(&project_path, rule) {
        Ok(()) => true,
        Err(e) => { eprintln!("Warning: memory.md write failed: {e}"); false }
    };

    if json {
        println!("{}", serde_json::json!({"status":"ok","rule":rule,"memory_written":memory_ok}));
    } else if memory_ok {
        println!("✓ Saved to ~/.agents/instincts.json");
        println!("✓ Appended to {}/memory.md", project_path.display());
    } else {
        println!("✓ Saved to ~/.agents/instincts.json only");
    }
}
```

- [ ] **Step 2.6: `cmd_instinct_list` ekle**

```rust
fn cmd_instinct_list(path: Option<std::path::PathBuf>, json: bool) {
    use crate::instinct::{load_project_rules, InstinctEngine};

    let engine = InstinctEngine::init();
    let global = &engine.data.learned_rules;
    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let project = load_project_rules(&project_path);

    if json {
        let out = serde_json::json!({"global": global, "project": project});
        match serde_json::to_string_pretty(&out) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
        return;
    }

    println!("Global instincts ({}):", global.len());
    if global.is_empty() { println!("  (none)"); }
    else { for r in global { println!("  - {r}"); } }

    println!("\nProject instincts ({}):", project.len());
    if project.is_empty() { println!("  (none)"); }
    else { for r in &project { println!("  - {r}"); } }
}
```

- [ ] **Step 2.7: `cmd_instinct_suggest` ekle**

```rust
fn cmd_instinct_suggest(project: Option<String>, dev_ops: &std::path::Path, _json: bool) {
    use crate::instinct::{append_to_memory_md, suggest_from_health, InstinctEngine};
    use crate::health::check_project;

    let projects = crate::entities::load_entities(dev_ops);
    let target = if let Some(ref name) = project {
        let n = name.to_lowercase();
        projects.into_iter().find(|p| p.name.to_lowercase().contains(&n))
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        projects.into_iter().find(|p| p.local_path == cwd)
    };

    let proj = match target {
        Some(p) => p,
        None => {
            eprintln!("Project not found. Try: raios instinct suggest <project-name>");
            std::process::exit(1);
        }
    };

    println!("Analyzing project: {}...", proj.name);
    let health = check_project(&proj);
    let suggestions = suggest_from_health(&health);

    if suggestions.is_empty() {
        println!("No suggestions — project looks healthy!");
        return;
    }

    println!("\nSuggested instincts:");
    for (i, s) in suggestions.iter().enumerate() {
        println!("  [{}] {}", i + 1, s);
    }

    print!("\nAccept? (y=all / 1,2=specific / n=none): ");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        eprintln!("Could not read input");
        return;
    }
    let input = input.trim().to_lowercase();

    let accepted: Vec<&String> = if input == "y" {
        suggestions.iter().collect()
    } else if input == "n" || input.is_empty() {
        vec![]
    } else {
        input.split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .filter(|&i| i >= 1 && i <= suggestions.len())
            .map(|i| &suggestions[i - 1])
            .collect()
    };

    if accepted.is_empty() {
        println!("No instincts added.");
        return;
    }

    let mut engine = InstinctEngine::init();
    for rule in &accepted {
        engine.add_rule((*rule).clone());
        match append_to_memory_md(&proj.local_path, rule) {
            Ok(()) => println!("✓ Saved: \"{}\"", rule),
            Err(e) => { eprintln!("Warning: memory.md: {e}"); println!("✓ JSON only: \"{}\"", rule); }
        }
    }
    if let Err(e) = engine.save() {
        eprintln!("Failed to save instincts.json: {e}");
    }
}
```

- [ ] **Step 2.8: Health footer ekle**

`cli.rs`'te `cmd_health` fonksiyonunu bul. Tek proje raporu basıldıktan sonra (JSON değilken) footer ekle:

```rust
if !json {
    let suggestions = crate::instinct::suggest_from_health(&health);
    if !suggestions.is_empty() {
        println!(
            "\n💡 {} instinct öneri — run: raios instinct suggest {}",
            suggestions.len(),
            health.name
        );
    }
}
```

`health` değişkeninin `ProjectHealth` tipinde olduğundan emin ol. Birden fazla proje döngüsü varsa her proje için ekle.

- [ ] **Step 2.9: Build**

```bash
cargo build --bin raios 2>&1 | head -40
```

- [ ] **Step 2.10: Tüm testleri çalıştır**

```bash
cargo test --lib 2>&1 | tail -5
```

Beklenen: `test result: ok. 75 passed`

- [ ] **Step 2.11: Smoke testler**

```bash
cargo run --bin raios -- instinct add "Test rule Faz 3" 2>&1
cargo run --bin raios -- instinct list 2>&1
```

- [ ] **Step 2.12: Final commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): raios instinct add/list/suggest + health footer"
```
