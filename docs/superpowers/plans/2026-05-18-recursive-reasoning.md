# Plan 5: Recursive Reasoning — Deep Task Decomposition

> **Subagent-Driven Development** ile execute et. alexzhang13/RLM'den ilham alındı.

**Goal:** Karmaşık görevleri (örn. "auth sistemini güvenli hale getir") otomatik olarak alt görevlere bölen, her alt görevi bağımsız çalıştıran ve sonuçları birleştiren bir sistem. RLM (Recursive Language Model) yaklaşımı.

**Philosophy:** "Bir görevi doğrudan çözmeye çalışma; önce onu çözülebilir parçalara ayır."

**Architecture:**
```
raios reason "auth sistemini güvenli hale getir"
    ↓
TaskDecomposer (LLM veya rule-based)
    ↓ [subtasks]
SubtaskQueue
    ├── "security scan yap" → raios security .
    ├── "auth modülünü incele" → agent: claude
    ├── "test coverage'ı kontrol et" → raios build --test
    └── "güvenlik önerilerini uygula" → agent: claude
    ↓
ResultAggregator
    ↓
FinalReport (TUI + MCP tool)
```

---

## Decomposition Stratejileri

### 1. Pattern-Based (MVP, hızlı)
Bilinen görev tipleri için statik şablonlar:
```
"güvenli hale getir" →
  1. security scan
  2. deps audit
  3. code review for auth
  4. fix critical issues

"refactor et" →
  1. refactor scan
  2. identify high-priority files
  3. extract functions
  4. test coverage check

"production'a hazırla" →
  1. build check
  2. test suite
  3. security scan
  4. env check
  5. git status
```

### 2. LLM-Based (Phase 2)
`raios reason` + Claude API ile dinamik decomposition. Şu formatta prompt:
```
System: You are a task decomposer. Break the task into atomic subtasks.
User: Task: {task}
      Project context: {health summary}
      Output JSON: {"subtasks": [{"description": "...", "type": "cli|agent", "command": "..."}]}
```

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/reasoning/mod.rs` | Yeni — `TaskDecomposer`, `SubtaskGraph` |
| `src/reasoning/patterns.rs` | Yeni — statik decomposition şablonları |
| `src/reasoning/executor.rs` | Yeni — parallel subtask execution |
| `src/reasoning/aggregator.rs` | Yeni — result aggregation + report |
| `src/cli.rs` | `Commands::Reason` |
| `src/lib.rs` | `pub mod reasoning;` |

---

## Task 1: Decomposition Patterns (MVP)

**Files:** `src/reasoning/patterns.rs`

- [ ] Oluştur:
```rust
use crate::reasoning::mod::Subtask;

pub fn decompose(task: &str) -> Vec<Subtask> {
    let lower = task.to_lowercase();

    if lower.contains("güvenli") || lower.contains("security") || lower.contains("secure") {
        return security_hardening_subtasks();
    }
    if lower.contains("refactor") || lower.contains("temizle") || lower.contains("clean") {
        return refactor_subtasks();
    }
    if lower.contains("production") || lower.contains("deploy") || lower.contains("yayınla") {
        return production_ready_subtasks();
    }
    if lower.contains("test") && (lower.contains("yaz") || lower.contains("ekle") || lower.contains("coverage")) {
        return test_coverage_subtasks();
    }
    if lower.contains("performance") || lower.contains("performans") || lower.contains("hızlandır") {
        return performance_subtasks();
    }
    // Fallback: single agent task
    vec![Subtask {
        description: task.to_string(),
        task_type: SubtaskType::Agent("claude".into()),
        command: None,
        depends_on: vec![],
    }]
}

fn security_hardening_subtasks() -> Vec<Subtask> {
    vec![
        Subtask::cli("OWASP security scan", "security ."),
        Subtask::cli("Dependency CVE audit", "deps"),
        Subtask::cli("Health compliance check", "health"),
        Subtask::agent("Review auth code and fix critical issues", "claude"),
        Subtask::agent("Verify fixes and write security tests", "claude"),
    ]
}

fn refactor_subtasks() -> Vec<Subtask> {
    vec![
        Subtask::cli("Scan refactor opportunities", "health"),
        Subtask::agent("Identify top 3 files to refactor", "claude"),
        Subtask::agent("Refactor high-priority files", "claude"),
        Subtask::cli("Verify tests still pass", "build --test"),
    ]
}

fn production_ready_subtasks() -> Vec<Subtask> {
    vec![
        Subtask::cli("Build check", "build"),
        Subtask::cli("Test suite", "build --test"),
        Subtask::cli("Security scan", "security ."),
        Subtask::cli("Env file check", "env"),
        Subtask::cli("Git status", "git status"),
        Subtask::cli("CI status", "ci"),
    ]
}

fn test_coverage_subtasks() -> Vec<Subtask> {
    vec![
        Subtask::cli("Current test status", "build --test"),
        Subtask::cli("Health + refactor check", "health"),
        Subtask::agent("Write missing unit tests for critical paths", "claude"),
        Subtask::agent("Write integration tests", "claude"),
        Subtask::cli("Verify coverage improved", "build --test"),
    ]
}

fn performance_subtasks() -> Vec<Subtask> {
    vec![
        Subtask::cli("Disk usage analysis", "disk"),
        Subtask::cli("Refactor scan (high complexity)", "health"),
        Subtask::agent("Profile and identify bottlenecks", "claude"),
        Subtask::agent("Optimize top 3 bottlenecks", "claude"),
    ]
}
```

- [ ] Commit: `feat(reasoning): decomposition pattern templates`

---

## Task 2: SubtaskGraph + Executor

**Files:** `src/reasoning/mod.rs`, `src/reasoning/executor.rs`

- [ ] `src/reasoning/mod.rs`:
```rust
pub mod aggregator;
pub mod executor;
pub mod patterns;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub description: String,
    pub task_type: SubtaskType,
    pub command: Option<String>,  // CLI command args
    pub depends_on: Vec<usize>,   // indices of prerequisite subtasks
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubtaskType {
    Cli,
    Agent(String), // agent name
}

impl Subtask {
    pub fn cli(description: &str, command: &str) -> Self {
        Self {
            description: description.into(),
            task_type: SubtaskType::Cli,
            command: Some(command.into()),
            depends_on: vec![],
        }
    }

    pub fn agent(description: &str, agent: &str) -> Self {
        Self {
            description: description.into(),
            task_type: SubtaskType::Agent(agent.into()),
            command: None,
            depends_on: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskResult {
    pub subtask: Subtask,
    pub output: String,
    pub success: bool,
    pub duration_ms: u64,
}
```

- [ ] `src/reasoning/executor.rs`:
```rust
use crate::reasoning::{Subtask, SubtaskResult, SubtaskType};
use std::path::Path;
use std::time::Instant;

pub fn execute_sequential(
    subtasks: &[Subtask],
    project_path: &Path,
    on_progress: impl Fn(usize, &str),
) -> Vec<SubtaskResult> {
    let mut results = Vec::new();

    for (i, subtask) in subtasks.iter().enumerate() {
        on_progress(i, &subtask.description);
        let start = Instant::now();

        let (output, success) = match &subtask.task_type {
            SubtaskType::Cli => run_cli_subtask(subtask.command.as_deref().unwrap_or(""), project_path),
            SubtaskType::Agent(agent) => run_agent_subtask(&subtask.description, agent, project_path),
        };

        results.push(SubtaskResult {
            subtask: subtask.clone(),
            output,
            success,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    results
}

fn run_cli_subtask(command: &str, cwd: &Path) -> (String, bool) {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() { return ("empty command".into(), false); }

    let output = std::process::Command::new("raios")
        .args(&parts)
        .current_dir(cwd)
        .output();

    match output {
        Ok(o) => (String::from_utf8_lossy(&o.stdout).to_string(), o.status.success()),
        Err(e) => (e.to_string(), false),
    }
}

fn run_agent_subtask(description: &str, agent: &str, cwd: &Path) -> (String, bool) {
    let output = std::process::Command::new("raios")
        .args(["task", description, "--agent", agent])
        .current_dir(cwd)
        .output();

    match output {
        Ok(o) => (String::from_utf8_lossy(&o.stdout).to_string(), o.status.success()),
        Err(e) => (e.to_string(), false),
    }
}
```

- [ ] Commit: `feat(reasoning): SubtaskGraph + sequential executor`

---

## Task 3: Result Aggregator + Report

**Files:** `src/reasoning/aggregator.rs`

- [ ] Oluştur:
```rust
use crate::reasoning::SubtaskResult;

pub struct ReasoningReport {
    pub task: String,
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
    } else {
        format!("⚠ {}/{} subtasks completed. {} failed.", succeeded, total, failed)
    };

    ReasoningReport { task: task.into(), total, succeeded, failed, total_duration_ms, summary, results }
}

pub fn print_report(report: &ReasoningReport) {
    println!("\n{}", "=".repeat(60));
    println!("REASONING REPORT: {}", report.task);
    println!("{}", "=".repeat(60));
    println!("{}", report.summary);
    println!("Duration: {:.1}s", report.total_duration_ms as f64 / 1000.0);
    println!();

    for (i, r) in report.results.iter().enumerate() {
        let icon = if r.success { "✓" } else { "✗" };
        println!("[{}] {} {} ({:.1}s)",
            i + 1, icon, r.subtask.description,
            r.duration_ms as f64 / 1000.0);
        if !r.success && !r.output.is_empty() {
            for line in r.output.lines().take(3) {
                println!("    {}", line);
            }
        }
    }
    println!("{}", "=".repeat(60));
}
```

- [ ] Commit: `feat(reasoning): result aggregator + terminal report`

---

## Task 4: `raios reason` CLI

**Files:** `src/cli.rs`, `src/lib.rs`

- [ ] `src/lib.rs`'e `pub mod reasoning;` ekle

- [ ] `Commands::Reason` ekle:
```rust
/// Deep task decomposition and recursive execution
Reason {
    /// High-level task description
    task: String,
    /// Dry run — show plan without executing
    #[arg(long)]
    dry_run: bool,
    /// Project path (default: current dir)
    #[arg(short, long)]
    project: Option<String>,
},
```

- [ ] `cmd_reason()`:
```rust
fn cmd_reason(task: &str, dry_run: bool, project: Option<String>, dev_ops: &Path, _json: bool) {
    use crate::reasoning::{aggregator, executor, patterns};

    let project_path = if let Some(name) = project {
        let projects = crate::entities::load_entities(dev_ops);
        let n = name.to_lowercase();
        projects.into_iter()
            .find(|p| p.name.to_lowercase().contains(&n))
            .map(|p| p.local_path)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    } else {
        std::env::current_dir().unwrap_or_default()
    };

    let subtasks = patterns::decompose(task);

    println!("Task: {}", task);
    println!("Decomposed into {} subtasks:", subtasks.len());
    for (i, s) in subtasks.iter().enumerate() {
        println!("  [{}] {}", i + 1, s.description);
    }

    if dry_run {
        println!("\n(Dry run — use without --dry-run to execute)");
        return;
    }

    println!("\nExecuting...\n");
    let results = executor::execute_sequential(&subtasks, &project_path, |i, desc| {
        println!("[{}/{}] {}...", i + 1, subtasks.len(), desc);
    });

    let report = aggregator::aggregate(task, results);
    aggregator::print_report(&report);
}
```

- [ ] Smoke test: `raios reason "güvenli hale getir" --dry-run`

- [ ] Commit: `feat(cli): raios reason — recursive task decomposition and execution`
