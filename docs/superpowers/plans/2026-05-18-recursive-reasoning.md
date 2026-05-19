# Plan 5: Recursive Reasoning — Deep Task Decomposition

> **For agentic workers:** Use superpowers:subagent-driven-development to execute task-by-task.

**Goal:** `raios reason "auth sistemini güvenli hale getir"` → otomatik alt görevlere böl → sırayla çalıştır → raporla. Pattern-based MVP önce; LLM-based decomposition Phase 2.

**Architecture:** `src/reasoning/` modülü: `patterns.rs` (şablonlar) + `executor.rs` (sequential runner) + `aggregator.rs` (rapor). CLI: `raios reason "task" [--dry-run]`.

**Tech Stack:** Rust, mevcut `core/build.rs`, `core/deps.rs`, `security.rs`, `health.rs`, `tasks.rs`, `entities.rs`

**Mevcut durum:**
- `tasks.rs::dispatch_to_agent(task, agent, path, errors)` → clipboard + Windows Terminal
- `core/build.rs::build(dir)` → `BuildResult { ok, .. }`
- `core/build.rs::test(dir)` → `TestResult { passed, failed, .. }`
- `core/deps.rs::check(dir)` → `DepsReport { outdated_count, cve_critical, .. }`
- `security.rs::scan_project(dir)` → `SecurityReport { score, grade, issues, .. }`
- `health.rs::check_project(proj)` → `ProjectHealth`

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/lib.rs` | `pub mod reasoning;` |
| `src/reasoning/mod.rs` | `Subtask`, `SubtaskType`, `SubtaskResult` |
| `src/reasoning/patterns.rs` | decompose() + 5 şablon |
| `src/reasoning/executor.rs` | execute_sequential() |
| `src/reasoning/aggregator.rs` | aggregate() + print_report() |
| `src/cli.rs` | `Commands::Reason` + `cmd_reason()` |

---

## Task 1: Module Scaffold + Core Types

**Files:** `src/lib.rs`, `src/reasoning/mod.rs`

- [ ] **Step 1.1: `src/lib.rs`'e modül ekle**

Mevcut `pub mod` listesine (alfabetik sıraya göre):
```rust
pub mod reasoning;
```

- [ ] **Step 1.2: `src/reasoning/mod.rs` oluştur**

```rust
pub mod aggregator;
pub mod executor;
pub mod patterns;

use serde::{Deserialize, Serialize};

/// A single atomic step in a decomposed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub description: String,
    pub task_type: SubtaskType,
    /// CLI args passed to `raios` (e.g. ["security", "."])
    pub cli_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubtaskType {
    /// Runs `raios <cli_args>` and captures output
    Cli,
    /// Dispatches to an AI agent via `raios task "<description>" --agent <name>`
    Agent(String),
}

/// The result of executing one subtask.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskResult {
    pub description: String,
    pub output: String,
    pub success: bool,
    pub duration_ms: u64,
}

impl Subtask {
    pub fn cli(description: &str, args: &[&str]) -> Self {
        Self {
            description: description.into(),
            task_type: SubtaskType::Cli,
            cli_args: Some(args.iter().map(|s| s.to_string()).collect()),
        }
    }

    pub fn agent(description: &str, agent: &str) -> Self {
        Self {
            description: description.into(),
            task_type: SubtaskType::Agent(agent.into()),
            cli_args: None,
        }
    }
}
```

- [ ] **Step 1.3: cargo check**

```bash
cargo check 2>&1 | head -10
```

- [ ] **Step 1.4: Commit**

```bash
git add src/lib.rs src/reasoning/mod.rs
git commit -m "feat(reasoning): module scaffold + Subtask/SubtaskType/SubtaskResult types"
```

---

## Task 2: Decomposition Patterns

**Files:** `src/reasoning/patterns.rs`

- [ ] **Step 2.1: Failing tests yaz**

`src/reasoning/patterns.rs` oluştur:

```rust
use crate::reasoning::mod::{Subtask, SubtaskType};

pub fn decompose(task: &str) -> Vec<Subtask> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_task_decomposes() {
        let subtasks = decompose("auth sistemini güvenli hale getir");
        assert!(!subtasks.is_empty());
        assert!(subtasks.iter().any(|s| matches!(s.task_type, SubtaskType::Cli)));
    }

    #[test]
    fn refactor_task_decomposes() {
        let subtasks = decompose("projeyi refactor et");
        assert!(!subtasks.is_empty());
    }

    #[test]
    fn production_task_decomposes() {
        let subtasks = decompose("production'a hazırla");
        assert!(subtasks.len() >= 4);
    }

    #[test]
    fn unknown_task_returns_single_agent() {
        let subtasks = decompose("xyzzy nonsense task abc");
        assert_eq!(subtasks.len(), 1);
        assert!(matches!(subtasks[0].task_type, SubtaskType::Agent(_)));
    }
}
```

- [ ] **Step 2.2: Testleri çalıştır — FAIL beklenir**

```bash
cargo test --lib reasoning::patterns::tests -- --nocapture 2>&1 | head -10
```

- [ ] **Step 2.3: `decompose()` implementasyonu yaz**

`todo!()` yerine:

```rust
pub fn decompose(task: &str) -> Vec<Subtask> {
    let lower = task.to_lowercase();

    if contains_any(&lower, &["güvenli", "security", "secure", "güvenlik", "owasp"]) {
        return security_hardening();
    }
    if contains_any(&lower, &["refactor", "temizle", "clean up", "yeniden yaz", "refaktör"]) {
        return refactor_flow();
    }
    if contains_any(&lower, &["production", "deploy", "yayınla", "canlıya", "release", "ship"]) {
        return production_readiness();
    }
    if contains_any(&lower, &["test yaz", "test ekle", "coverage", "test coverage", "unit test"]) {
        return test_coverage();
    }
    if contains_any(&lower, &["performans", "performance", "hızlandır", "optimize", "yavaş"]) {
        return performance_audit();
    }

    // Fallback: single agent task
    vec![Subtask::agent(task, "claude")]
}

fn contains_any(s: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|k| s.contains(k))
}

fn security_hardening() -> Vec<Subtask> {
    vec![
        Subtask::cli("OWASP security scan", &["security", "."]),
        Subtask::cli("Dependency CVE audit", &["deps"]),
        Subtask::cli("Project health & compliance check", &["health"]),
        Subtask::agent(
            "Review the security scan results and fix all CRITICAL and HIGH issues",
            "claude",
        ),
        Subtask::agent(
            "Write security tests for the fixed vulnerabilities and verify no regressions",
            "claude",
        ),
    ]
}

fn refactor_flow() -> Vec<Subtask> {
    vec![
        Subtask::cli("Run refactor scan to find high-priority files", &["health"]),
        Subtask::agent(
            "Identify the top 3 files with the highest complexity and nesting depth",
            "claude",
        ),
        Subtask::agent(
            "Refactor the identified files: extract functions, reduce nesting, improve naming",
            "claude",
        ),
        Subtask::cli("Verify all tests still pass after refactoring", &["build", "--test"]),
    ]
}

fn production_readiness() -> Vec<Subtask> {
    vec![
        Subtask::cli("Build check", &["build"]),
        Subtask::cli("Full test suite", &["build", "--test"]),
        Subtask::cli("Security scan", &["security", "."]),
        Subtask::cli("Dependency audit", &["deps"]),
        Subtask::cli("Environment file check", &["env"]),
        Subtask::cli("Git status check", &["git", "status"]),
        Subtask::cli("CI/CD pipeline status", &["ci"]),
    ]
}

fn test_coverage() -> Vec<Subtask> {
    vec![
        Subtask::cli("Current test status", &["build", "--test"]),
        Subtask::cli("Code complexity scan (find untested hot spots)", &["health"]),
        Subtask::agent(
            "Identify the 5 most critical functions/modules that lack test coverage",
            "claude",
        ),
        Subtask::agent(
            "Write comprehensive unit tests for the identified critical paths",
            "claude",
        ),
        Subtask::agent(
            "Write integration tests for the main user flows",
            "claude",
        ),
        Subtask::cli("Verify test coverage improved", &["build", "--test"]),
    ]
}

fn performance_audit() -> Vec<Subtask> {
    vec![
        Subtask::cli("Project size and disk usage analysis", &["disk"]),
        Subtask::cli("Code complexity scan", &["health"]),
        Subtask::cli("Dependency check (unused or heavy deps)", &["deps"]),
        Subtask::agent(
            "Profile the application and identify the top 3 performance bottlenecks",
            "claude",
        ),
        Subtask::agent(
            "Optimize the identified bottlenecks with measurable improvements",
            "claude",
        ),
    ]
}
```

- [ ] **Step 2.4: Testleri çalıştır — PASS beklenir**

```bash
cargo test --lib reasoning::patterns::tests -- --nocapture
```

Beklenen: `test result: ok. 4 passed`

- [ ] **Step 2.5: Commit**

```bash
git add src/reasoning/patterns.rs
git commit -m "feat(reasoning): decompose() with 5 patterns + 4 tests"
```

---

## Task 3: Sequential Executor

**Files:** `src/reasoning/executor.rs`

- [ ] **Step 3.1: `src/reasoning/executor.rs` oluştur**

```rust
use crate::reasoning::mod::{Subtask, SubtaskResult, SubtaskType};
use std::path::Path;
use std::time::Instant;

/// Execute subtasks one by one, calling `on_progress` before each.
/// Returns results for all subtasks, including failed ones (never aborts early).
pub fn execute_sequential(
    subtasks: &[Subtask],
    project_path: &Path,
    on_progress: impl Fn(usize, usize, &str),
) -> Vec<SubtaskResult> {
    let total = subtasks.len();
    let mut results = Vec::with_capacity(total);

    for (i, subtask) in subtasks.iter().enumerate() {
        on_progress(i + 1, total, &subtask.description);
        let start = Instant::now();

        let (output, success) = match &subtask.task_type {
            SubtaskType::Cli => run_cli(subtask.cli_args.as_deref().unwrap_or(&[]), project_path),
            SubtaskType::Agent(agent) => run_agent(&subtask.description, agent, project_path),
        };

        results.push(SubtaskResult {
            description: subtask.description.clone(),
            output,
            success,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    results
}

fn run_cli(args: &[String], cwd: &Path) -> (String, bool) {
    if args.is_empty() {
        return ("empty args".into(), false);
    }

    // Handle --test flag: raios build --test maps to "raios build" with --test
    let bin = std::process::Command::new("raios")
        .args(args)
        .current_dir(cwd)
        .output();

    match bin {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout
            } else if stdout.is_empty() {
                stderr
            } else {
                format!("{}\n{}", stdout, stderr)
            };
            (combined.trim().to_string(), out.status.success())
        }
        Err(e) => (format!("Failed to run raios: {}", e), false),
    }
}

fn run_agent(description: &str, agent: &str, cwd: &Path) -> (String, bool) {
    let task = crate::tasks::Task {
        text: description.to_string(),
        completed: false,
        agent: Some(agent.to_string()),
        project: None,
    };

    let result = crate::tasks::dispatch_to_agent(&task, agent, Some(&cwd.to_path_buf()), None);
    let success = !result.to_lowercase().contains("error")
        && !result.to_lowercase().contains("failed");
    (result, success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning::mod::{Subtask, SubtaskType};

    #[test]
    fn execute_empty_returns_empty() {
        let results = execute_sequential(&[], std::path::Path::new("."), |_, _, _| {});
        assert!(results.is_empty());
    }

    #[test]
    fn progress_callback_called_for_each() {
        use std::sync::{Arc, Mutex};
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_clone = calls.clone();
        let subtasks = vec![
            Subtask::cli("step 1", &["health"]),
            Subtask::cli("step 2", &["health"]),
        ];
        execute_sequential(&subtasks, std::path::Path::new("."), move |i, total, desc| {
            calls_clone.lock().unwrap().push((i, total, desc.to_string()));
        });
        let c = calls.lock().unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].0, 1);
        assert_eq!(c[1].0, 2);
        assert_eq!(c[0].1, 2);
    }
}
```

- [ ] **Step 3.2: Testleri çalıştır**

```bash
cargo test --lib reasoning::executor::tests -- --nocapture
```

Beklenen: `test result: ok. 2 passed`

- [ ] **Step 3.3: Commit**

```bash
git add src/reasoning/executor.rs
git commit -m "feat(reasoning): sequential executor with progress callback + 2 tests"
```

---

## Task 4: Result Aggregator

**Files:** `src/reasoning/aggregator.rs`

- [ ] **Step 4.1: `src/reasoning/aggregator.rs` oluştur**

```rust
use crate::reasoning::mod::SubtaskResult;

pub struct ReasoningReport {
    pub original_task: String,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub total_duration_ms: u64,
    pub summary: String,
    pub results: Vec<SubtaskResult>,
}

pub fn aggregate(task: &str, results: Vec<SubtaskResult>) -> ReasoningReport {
    let total = results.len();
    let succeeded = results.iter().filter(|r| r.success).count();
    let failed = total - succeeded;
    let total_duration_ms: u64 = results.iter().map(|r| r.duration_ms).sum();

    let summary = if failed == 0 {
        format!("✓ All {} subtasks completed successfully", total)
    } else if succeeded == 0 {
        format!("✗ All {} subtasks failed", total)
    } else {
        format!(
            "⚠ {}/{} subtasks completed — {} failed",
            succeeded, total, failed
        )
    };

    ReasoningReport {
        original_task: task.to_string(),
        total,
        succeeded,
        failed,
        total_duration_ms,
        summary,
        results,
    }
}

pub fn print_report(report: &ReasoningReport) {
    let separator = "═".repeat(64);
    println!("\n{}", separator);
    println!("REASONING REPORT");
    println!("Task:     {}", report.original_task);
    println!("Result:   {}", report.summary);
    println!(
        "Duration: {:.1}s  ({} subtasks)",
        report.total_duration_ms as f64 / 1000.0,
        report.total
    );
    println!("{}", separator);

    for (i, r) in report.results.iter().enumerate() {
        let icon = if r.success { "✓" } else { "✗" };
        println!(
            "\n[{}/{}] {} {} ({:.1}s)",
            i + 1,
            report.total,
            icon,
            r.description,
            r.duration_ms as f64 / 1000.0
        );
        if !r.output.is_empty() {
            // Show first 5 lines of output; truncate the rest
            let lines: Vec<&str> = r.output.lines().collect();
            let show = lines.len().min(5);
            for line in &lines[..show] {
                println!("    {}", line);
            }
            if lines.len() > 5 {
                println!("    ... ({} more lines)", lines.len() - 5);
            }
        }
    }

    println!("\n{}", separator);
    if report.failed > 0 {
        println!("Failed subtasks:");
        for r in report.results.iter().filter(|r| !r.success) {
            println!("  ✗ {}", r.description);
        }
    }
}

pub fn to_json(report: &ReasoningReport) -> serde_json::Value {
    serde_json::json!({
        "task": report.original_task,
        "summary": report.summary,
        "total": report.total,
        "succeeded": report.succeeded,
        "failed": report.failed,
        "duration_ms": report.total_duration_ms,
        "subtasks": report.results.iter().map(|r| serde_json::json!({
            "description": r.description,
            "success": r.success,
            "duration_ms": r.duration_ms,
            "output": r.output.lines().take(10).collect::<Vec<_>>().join("\n")
        })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning::mod::SubtaskResult;

    fn make_result(desc: &str, success: bool, ms: u64) -> SubtaskResult {
        SubtaskResult {
            description: desc.to_string(),
            output: String::new(),
            success,
            duration_ms: ms,
        }
    }

    #[test]
    fn all_success_summary() {
        let results = vec![
            make_result("step 1", true, 100),
            make_result("step 2", true, 200),
        ];
        let report = aggregate("test task", results);
        assert_eq!(report.succeeded, 2);
        assert_eq!(report.failed, 0);
        assert!(report.summary.contains("All 2 subtasks"));
    }

    #[test]
    fn partial_failure_summary() {
        let results = vec![
            make_result("step 1", true, 100),
            make_result("step 2", false, 200),
            make_result("step 3", true, 150),
        ];
        let report = aggregate("partial task", results);
        assert_eq!(report.succeeded, 2);
        assert_eq!(report.failed, 1);
        assert!(report.summary.contains("2/3"));
    }

    #[test]
    fn total_duration_sums_correctly() {
        let results = vec![
            make_result("a", true, 1000),
            make_result("b", true, 2000),
            make_result("c", false, 3000),
        ];
        let report = aggregate("dur test", results);
        assert_eq!(report.total_duration_ms, 6000);
    }
}
```

- [ ] **Step 4.2: Testleri çalıştır**

```bash
cargo test --lib reasoning::aggregator::tests -- --nocapture
```

Beklenen: `test result: ok. 3 passed`

- [ ] **Step 4.3: Commit**

```bash
git add src/reasoning/aggregator.rs
git commit -m "feat(reasoning): aggregator with print_report + to_json + 3 tests"
```

---

## Task 5: `raios reason` CLI Komutu

**Files:** `src/cli.rs`

- [ ] **Step 5.1: `Commands::Reason` varyantı ekle**

Mevcut son varyanttan sonra:

```rust
/// Deep task decomposition and recursive execution
/// Examples: "auth güvenli hale getir", "refactor et", "production'a hazırla"
Reason {
    /// High-level task description (Turkish or English)
    task: String,
    /// Show decomposition plan without executing
    #[arg(long)]
    dry_run: bool,
    /// Project name or path (default: current directory)
    #[arg(short, long)]
    project: Option<String>,
},
```

- [ ] **Step 5.2: Match arm ekle**

```rust
Commands::Reason { task, dry_run, project } => {
    cmd_reason(&task, dry_run, project, &cfg.dev_ops_path, cli.json);
}
```

- [ ] **Step 5.3: `cmd_reason()` fonksiyonu ekle**

```rust
fn cmd_reason(
    task: &str,
    dry_run: bool,
    project: Option<String>,
    dev_ops: &std::path::Path,
    json: bool,
) {
    use crate::reasoning::{aggregator, executor, patterns};

    // Resolve project path
    let project_path = if let Some(ref name) = project {
        let projects = crate::entities::load_entities(dev_ops);
        let n = name.to_lowercase();
        projects
            .into_iter()
            .find(|p| {
                p.name.to_lowercase().contains(&n)
                    || p.local_path.to_string_lossy().to_lowercase().contains(&n)
            })
            .map(|p| p.local_path)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    } else {
        std::env::current_dir().unwrap_or_default()
    };

    // Decompose
    let subtasks = patterns::decompose(task);

    if dry_run || (!json && subtasks.len() > 0) {
        println!("Task: {}", task);
        println!("Project: {}", project_path.display());
        println!("Decomposed into {} subtask(s):", subtasks.len());
        for (i, s) in subtasks.iter().enumerate() {
            let kind = match &s.task_type {
                crate::reasoning::mod::SubtaskType::Cli => "CLI".to_string(),
                crate::reasoning::mod::SubtaskType::Agent(a) => format!("Agent({})", a),
            };
            println!("  [{}/{}] [{}] {}", i + 1, subtasks.len(), kind, s.description);
        }

        if dry_run {
            println!("\n(Dry run — remove --dry-run to execute)");
            return;
        }
        println!();
    }

    // Execute
    let results = executor::execute_sequential(&subtasks, &project_path, |i, total, desc| {
        if !json {
            println!("[{}/{}] {}...", i, total, desc);
        }
    });

    // Aggregate
    let report = aggregator::aggregate(task, results);

    if json {
        match serde_json::to_string_pretty(&aggregator::to_json(&report)) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
    } else {
        aggregator::print_report(&report);
    }
}
```

- [ ] **Step 5.4: cargo build**

```bash
cargo build --bin raios 2>&1 | tail -5
```

Beklenen: `Finished` satırı.

- [ ] **Step 5.5: Smoke testler**

```bash
cargo run --bin raios -- reason "auth sistemini güvenli hale getir" --dry-run
```

Beklenen çıktı:
```
Task: auth sistemini güvenli hale getir
Project: C:\...
Decomposed into 5 subtask(s):
  [1/5] [CLI] OWASP security scan
  [2/5] [CLI] Dependency CVE audit
  [3/5] [CLI] Project health & compliance check
  [4/5] [Agent(claude)] Review the security scan results...
  [5/5] [Agent(claude)] Write security tests...

(Dry run — remove --dry-run to execute)
```

```bash
cargo run --bin raios -- reason "xyzzy nonsense" --dry-run
```

Beklenen:
```
Task: xyzzy nonsense
Decomposed into 1 subtask(s):
  [1/1] [Agent(claude)] xyzzy nonsense

(Dry run — remove --dry-run to execute)
```

- [ ] **Step 5.6: Tüm testleri çalıştır**

```bash
cargo test --lib 2>&1 | grep "test result"
```

Beklenen: tüm testler geçiyor (mevcut 83 + yeni 9 = ~92 test).

- [ ] **Step 5.7: Final commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): raios reason — recursive task decomposition with dry-run support"
```
