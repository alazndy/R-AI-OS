# Plan 4: Evolutionary Intelligence — Autonomous Instinct Refinement

> **For agentic workers:** Use superpowers:subagent-driven-development to execute task-by-task.

**Goal:** Ajanlar task tamamladıkça sonuçlar DB'ye kaydedilir. `aiosd` içindeki `InstinctRefinementWorker` her gün birikmiş sonuçları analiz eder, yüksek güvenli instinct'leri otomatik ekler, düşük güvenli olanları review kuyruğuna alır.

**Architecture:** `task_outcomes` SQLite tablosu → `TaskOutcome` logging in `tasks.rs` → `refine_instincts()` pattern detector in `instinct.rs` → `InstinctRefinementWorker` in `aiosd` → `raios instinct review` CLI.

**Tech Stack:** Rust, mevcut `SQLite/db.rs`, `instinct.rs`, `tasks.rs`, `daemon/server.rs`

**Mevcut durum:**
- `instinct.rs`: `InstinctEngine`, `suggest_from_health()`, `append_to_memory_md()`, `load_project_rules()` var
- `tasks.rs`: `Task { text, completed, agent, project }`, `dispatch_to_agent()` var — outcome logging YOK
- `db.rs` `migrate()`: idempotent ALTER pattern mevcut (satır ~30)
- `daemon/server.rs`: worker spawn pattern mevcut (satır ~40-70)
- `daemon/state.rs`: `DaemonState` — `active_swarms` (Plan 2'den sonra) dahil

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/db.rs` | `task_outcomes` tablosu + `log_task_outcome()` + `get_outcomes_since()` |
| `src/tasks.rs` | `TaskOutcome` struct + outcome logging in `dispatch_to_agent()` |
| `src/instinct.rs` | `InstinctCandidate` struct + `refine_instincts()` pattern detectors |
| `src/daemon/refinement.rs` | Yeni — `start_refinement_worker()` async fn |
| `src/daemon/mod.rs` | `pub mod refinement;` |
| `src/daemon/server.rs` | refinement worker spawn |
| `src/cli.rs` | `InstinctCmd::Review` varyantı + `cmd_instinct_review()` |

---

## Task 1: `task_outcomes` DB Tablosu

**Files:** `src/db.rs`

- [ ] **Step 1.1: `TaskOutcome` struct ekle**

`src/db.rs`'in başına (mevcut `use` bloğundan sonra, diğer struct'lardan önce):

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskOutcome {
    pub task_id: String,
    pub project: String,
    pub agent: String,
    pub description: String,
    pub success: bool,
    pub duration_ms: Option<i64>,
    pub error_type: Option<String>, // "build_fail" | "test_fail" | "timeout" | "agent_error"
    pub files_changed: usize,
}
```

- [ ] **Step 1.2: `migrate()`'e tablo oluşturma ekle**

`src/db.rs` satır ~30 `fn migrate()` içinde, mevcut son `let _ = conn.execute_batch(...)` satırından sonra:

```rust
    // Task outcome tracking for evolutionary intelligence
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS task_outcomes (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id       TEXT NOT NULL,
            project       TEXT NOT NULL DEFAULT '',
            agent         TEXT NOT NULL DEFAULT 'unknown',
            description   TEXT NOT NULL DEFAULT '',
            success       INTEGER NOT NULL DEFAULT 0,
            duration_ms   INTEGER,
            error_type    TEXT,
            files_changed INTEGER NOT NULL DEFAULT 0,
            created_at    TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_task_outcomes_project ON task_outcomes(project);
        CREATE INDEX IF NOT EXISTS idx_task_outcomes_created ON task_outcomes(created_at);
    ")?;
```

- [ ] **Step 1.3: `log_task_outcome()` fonksiyonu ekle**

`src/db.rs`'deki public API fonksiyonları arasına:

```rust
pub fn log_task_outcome(conn: &Connection, o: &TaskOutcome) -> Result<()> {
    conn.execute(
        "INSERT INTO task_outcomes
            (task_id, project, agent, description, success, duration_ms, error_type, files_changed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            o.task_id,
            o.project,
            o.agent,
            o.description,
            o.success as i64,
            o.duration_ms,
            o.error_type,
            o.files_changed as i64,
        ],
    )?;
    Ok(())
}
```

- [ ] **Step 1.4: `get_outcomes_since()` fonksiyonu ekle**

```rust
/// Return all task outcomes from the last `hours` hours.
pub fn get_outcomes_since(conn: &Connection, hours: i64) -> Result<Vec<TaskOutcome>> {
    let mut stmt = conn.prepare(
        "SELECT task_id, project, agent, description, success,
                duration_ms, error_type, files_changed
         FROM task_outcomes
         WHERE created_at >= datetime('now', ?1)
         ORDER BY created_at DESC",
    )?;

    let modifier = format!("-{} hours", hours);
    let rows = stmt.query_map(rusqlite::params![modifier], |row| {
        Ok(TaskOutcome {
            task_id: row.get(0)?,
            project: row.get(1)?,
            agent: row.get(2)?,
            description: row.get(3)?,
            success: row.get::<_, i64>(4)? != 0,
            duration_ms: row.get(5)?,
            error_type: row.get(6)?,
            files_changed: row.get::<_, i64>(7)? as usize,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}
```

- [ ] **Step 1.5: cargo check**

```bash
cargo check 2>&1 | head -20
```

- [ ] **Step 1.6: Commit**

```bash
git add src/db.rs
git commit -m "feat(db): task_outcomes table + log_task_outcome + get_outcomes_since"
```

---

## Task 2: Task Outcome Logging

**Files:** `src/tasks.rs`

- [ ] **Step 2.1: `dispatch_to_agent()` sonuna outcome logging ekle**

`src/tasks.rs`'deki `dispatch_to_agent()` fonksiyonunu oku. Fonksiyonun `return result;` satırından **önce** şunu ekle:

```rust
    // Log outcome for evolutionary intelligence
    let outcome = crate::db::TaskOutcome {
        task_id: uuid::Uuid::new_v4().to_string(),
        project: task.project.clone().unwrap_or_default(),
        agent: agent.to_string(),
        description: task.text.chars().take(200).collect(),
        success: !result.contains("error") && !result.contains("failed") && !result.contains("Error"),
        duration_ms: None, // dispatch_to_agent doesn't track duration currently
        error_type: if result.to_lowercase().contains("timeout") {
            Some("timeout".to_string())
        } else if result.to_lowercase().contains("build") && result.to_lowercase().contains("fail") {
            Some("build_fail".to_string())
        } else if result.to_lowercase().contains("test") && result.to_lowercase().contains("fail") {
            Some("test_fail".to_string())
        } else {
            None
        },
        files_changed: 0,
    };
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::log_task_outcome(&conn, &outcome);
    }
```

- [ ] **Step 2.2: cargo check**

```bash
cargo check 2>&1 | head -20
```

- [ ] **Step 2.3: Commit**

```bash
git add src/tasks.rs
git commit -m "feat(tasks): log task outcomes to DB after dispatch_to_agent"
```

---

## Task 3: Pattern Detectors + `refine_instincts()`

**Files:** `src/instinct.rs`

- [ ] **Step 3.1: `InstinctCandidate` struct ekle**

`src/instinct.rs`'in başına (mevcut `use` bloğundan sonra):

```rust
#[derive(Debug, Clone)]
pub struct InstinctCandidate {
    pub rule: String,
    pub confidence: f32,  // 0.0–1.0
    pub evidence: String,
    pub project: Option<String>,
}
```

- [ ] **Step 3.2: `refine_instincts()` fonksiyonu ekle**

`load_project_rules()` fonksiyonundan sonra:

```rust
/// Analyze task outcomes and generate instinct candidates.
/// High-confidence (>0.85) candidates should be auto-approved.
/// Low-confidence candidates should go to review queue.
pub fn refine_instincts(outcomes: &[crate::db::TaskOutcome]) -> Vec<InstinctCandidate> {
    let mut candidates = Vec::new();

    if outcomes.is_empty() {
        return candidates;
    }

    // Pattern 1: High build failure rate (≥3 failures, >60% fail rate)
    let build_fails: Vec<_> = outcomes
        .iter()
        .filter(|o| o.error_type.as_deref() == Some("build_fail"))
        .collect();
    if build_fails.len() >= 3 {
        let fail_count = build_fails.iter().filter(|o| !o.success).count();
        let fail_rate = fail_count as f32 / build_fails.len() as f32;
        if fail_rate > 0.6 {
            // Group by project
            let mut by_project: std::collections::HashMap<&str, Vec<_>> = std::collections::HashMap::new();
            for o in &build_fails {
                by_project.entry(o.project.as_str()).or_default().push(o);
            }
            for (proj, group) in &by_project {
                let project_fails = group.iter().filter(|o| !o.success).count();
                if project_fails >= 2 {
                    candidates.push(InstinctCandidate {
                        rule: format!(
                            "Run `cargo check` or `npm run build` before dispatching tasks to {}",
                            proj
                        ),
                        confidence: project_fails as f32 / group.len() as f32,
                        evidence: format!(
                            "{}/{} build tasks failed in this project",
                            project_fails, group.len()
                        ),
                        project: Some(proj.to_string()),
                    });
                }
            }
        }
    }

    // Pattern 2: Frequent timeouts (≥2 timeout events)
    let timeouts: Vec<_> = outcomes
        .iter()
        .filter(|o| o.error_type.as_deref() == Some("timeout"))
        .collect();
    if timeouts.len() >= 2 {
        candidates.push(InstinctCandidate {
            rule: "Tasks frequently timeout — break complex tasks into smaller subtasks before dispatching".into(),
            confidence: (timeouts.len() as f32 / outcomes.len() as f32).min(0.95),
            evidence: format!("{}/{} tasks timed out", timeouts.len(), outcomes.len()),
            project: None,
        });
    }

    // Pattern 3: Agent preference — one agent has ≥90% success rate with ≥5 tasks
    for agent in &["claude", "gemini", "codex"] {
        let agent_tasks: Vec<_> = outcomes
            .iter()
            .filter(|o| o.agent == *agent)
            .collect();
        if agent_tasks.len() >= 5 {
            let success_count = agent_tasks.iter().filter(|o| o.success).count();
            let rate = success_count as f32 / agent_tasks.len() as f32;
            if rate >= 0.9 {
                // Group by project to find project-specific preference
                let mut by_project: std::collections::HashMap<&str, (usize, usize)> =
                    std::collections::HashMap::new();
                for o in &agent_tasks {
                    let entry = by_project.entry(o.project.as_str()).or_insert((0, 0));
                    entry.0 += 1;
                    if o.success { entry.1 += 1; }
                }
                for (proj, (total, successes)) in &by_project {
                    if *total >= 3 {
                        let proj_rate = *successes as f32 / *total as f32;
                        if proj_rate >= 0.9 {
                            candidates.push(InstinctCandidate {
                                rule: format!(
                                    "Prefer {} for tasks in {} — {:.0}% success rate ({}/{} tasks)",
                                    agent, proj, proj_rate * 100.0, successes, total
                                ),
                                confidence: proj_rate,
                                evidence: format!(
                                    "{} succeeded {}/{} tasks in {}",
                                    agent, successes, total, proj
                                ),
                                project: Some(proj.to_string()),
                            });
                        }
                    }
                }
            }
        }
    }

    // Pattern 4: Test failure pattern (≥3 test failures, no security issues)
    let test_fails: Vec<_> = outcomes
        .iter()
        .filter(|o| o.error_type.as_deref() == Some("test_fail"))
        .collect();
    if test_fails.len() >= 3 {
        candidates.push(InstinctCandidate {
            rule: "Write tests before implementing features — test failures are frequent in this workspace".into(),
            confidence: 0.75,
            evidence: format!("{} test failure events in the last analysis period", test_fails.len()),
            project: None,
        });
    }

    candidates
}
```

- [ ] **Step 3.3: Unit tests ekle**

`src/instinct.rs`'in sonuna:

```rust
#[cfg(test)]
mod refinement_tests {
    use super::*;
    use crate::db::TaskOutcome;

    fn make_outcome(agent: &str, project: &str, success: bool, error: Option<&str>) -> TaskOutcome {
        TaskOutcome {
            task_id: uuid::Uuid::new_v4().to_string(),
            project: project.to_string(),
            agent: agent.to_string(),
            description: "test task".to_string(),
            success,
            duration_ms: Some(5000),
            error_type: error.map(str::to_string),
            files_changed: 0,
        }
    }

    #[test]
    fn detects_build_failure_pattern() {
        let outcomes = vec![
            make_outcome("claude", "MyProj", false, Some("build_fail")),
            make_outcome("claude", "MyProj", false, Some("build_fail")),
            make_outcome("claude", "MyProj", false, Some("build_fail")),
        ];
        let candidates = refine_instincts(&outcomes);
        assert!(!candidates.is_empty(), "Should detect build failure pattern");
        assert!(candidates[0].rule.contains("cargo check") || candidates[0].rule.contains("build"));
    }

    #[test]
    fn detects_timeout_pattern() {
        let outcomes = vec![
            make_outcome("claude", "P1", false, Some("timeout")),
            make_outcome("gemini", "P2", false, Some("timeout")),
        ];
        let candidates = refine_instincts(&outcomes);
        assert!(candidates.iter().any(|c| c.rule.contains("timeout")));
    }

    #[test]
    fn empty_outcomes_returns_empty() {
        let candidates = refine_instincts(&[]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn detects_agent_preference() {
        let mut outcomes = Vec::new();
        for _ in 0..6 {
            outcomes.push(make_outcome("claude", "BestProj", true, None));
        }
        outcomes.push(make_outcome("claude", "BestProj", false, None));
        let candidates = refine_instincts(&outcomes);
        assert!(candidates.iter().any(|c| c.rule.contains("claude") && c.rule.contains("BestProj")));
    }
}
```

- [ ] **Step 3.4: Testleri çalıştır**

```bash
cargo test --lib instinct::refinement_tests -- --nocapture
```

Beklenen: `test result: ok. 4 passed`

- [ ] **Step 3.5: Commit**

```bash
git add src/instinct.rs
git commit -m "feat(instinct): InstinctCandidate + refine_instincts() with 4 pattern detectors + tests"
```

---

## Task 4: `InstinctRefinementWorker` in aiosd

**Files:** `src/daemon/refinement.rs`, `src/daemon/mod.rs`, `src/daemon/server.rs`

- [ ] **Step 4.1: `src/daemon/mod.rs`'e ekle**

Mevcut `pub mod` listesine:
```rust
pub mod refinement;
```

- [ ] **Step 4.2: `src/daemon/refinement.rs` oluştur**

```rust
use crate::instinct::{refine_instincts, InstinctCandidate, InstinctEngine};
use tokio::time::{interval, Duration};

/// Runs once per day. Analyzes task outcomes from the last 7 days.
/// Auto-approves high-confidence (>0.85) candidates.
/// Broadcasts pending review candidates to connected clients.
pub async fn start_refinement_worker(tx: tokio::sync::broadcast::Sender<String>) {
    let mut ticker = interval(Duration::from_secs(86_400)); // 24h
    loop {
        ticker.tick().await;
        run_cycle(&tx).await;
    }
}

async fn run_cycle(tx: &tokio::sync::broadcast::Sender<String>) {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[refinement] DB open failed: {e}");
            return;
        }
    };

    let outcomes = match crate::db::get_outcomes_since(&conn, 168) {
        // 7 days
        Ok(o) => o,
        Err(e) => {
            eprintln!("[refinement] Failed to load outcomes: {e}");
            return;
        }
    };

    if outcomes.is_empty() {
        return;
    }

    let candidates = refine_instincts(&outcomes);
    if candidates.is_empty() {
        return;
    }

    let mut engine = InstinctEngine::init();
    let mut auto_added: Vec<&InstinctCandidate> = Vec::new();
    let mut pending: Vec<&InstinctCandidate> = Vec::new();

    for c in &candidates {
        if c.confidence > 0.85 {
            engine.add_rule(c.rule.clone());
            auto_added.push(c);
        } else {
            pending.push(c);
        }
    }

    if !auto_added.is_empty() {
        if let Err(e) = engine.save() {
            eprintln!("[refinement] Failed to save instincts: {e}");
        } else {
            println!(
                "[refinement] Auto-added {} high-confidence instinct(s)",
                auto_added.len()
            );
        }
    }

    // Broadcast to VS Code / TUI so user can review low-confidence candidates
    let event = serde_json::json!({
        "event": "InstinctRefinement",
        "auto_added": auto_added.len(),
        "pending_review": pending.len(),
        "candidates": pending.iter().map(|c| serde_json::json!({
            "rule": c.rule,
            "confidence": c.confidence,
            "evidence": c.evidence,
            "project": c.project
        })).collect::<Vec<_>>()
    });

    let _ = tx.send(event.to_string());
}
```

- [ ] **Step 4.3: Worker'ı server.rs'de spawn et**

`src/daemon/server.rs`'deki diğer worker spawn'larından (satır ~40-70) sonra:

```rust
        let refinement_tx = tx.clone();
        tokio::spawn(async move {
            crate::daemon::refinement::start_refinement_worker(refinement_tx).await;
        });
```

- [ ] **Step 4.4: cargo check**

```bash
cargo check 2>&1 | head -20
```

- [ ] **Step 4.5: Commit**

```bash
git add src/daemon/refinement.rs src/daemon/mod.rs src/daemon/server.rs
git commit -m "feat(daemon): InstinctRefinementWorker — daily auto-learning cycle"
```

---

## Task 5: `raios instinct review` CLI

**Files:** `src/cli.rs`

- [ ] **Step 5.1: `InstinctCmd` enum'una `Review` ekle**

`src/cli.rs`'teki `InstinctCmd` enum'unda `Suggest { project }` varyantından sonra:

```rust
    /// Review pending instinct candidates from evolutionary learning
    Review {
        /// Auto-approve all pending candidates
        #[arg(long)]
        auto: bool,
    },
```

- [ ] **Step 5.2: `cmd_instinct` match arm güncelle**

```rust
InstinctCmd::Review { auto } => cmd_instinct_review(auto, json),
```

- [ ] **Step 5.3: `cmd_instinct_review()` fonksiyonu ekle**

```rust
fn cmd_instinct_review(auto: bool, json: bool) {
    use crate::instinct::{refine_instincts, InstinctEngine};

    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("DB error: {e}");
            return;
        }
    };

    let outcomes = match crate::db::get_outcomes_since(&conn, 168) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Could not load outcomes: {e}");
            return;
        }
    };

    if outcomes.is_empty() {
        if json {
            println!("{}", serde_json::json!({"candidates": [], "message": "No task outcomes in last 7 days"}));
        } else {
            println!("No task outcomes in the last 7 days. Run some tasks first.");
        }
        return;
    }

    let candidates = refine_instincts(&outcomes);
    if candidates.is_empty() {
        if json {
            println!("{}", serde_json::json!({"candidates": [], "message": "No new instinct candidates"}));
        } else {
            println!("No new instinct candidates. System looks healthy.");
        }
        return;
    }

    if json {
        let out: Vec<serde_json::Value> = candidates
            .iter()
            .map(|c| serde_json::json!({
                "rule": c.rule,
                "confidence": c.confidence,
                "evidence": c.evidence,
                "project": c.project
            }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    println!("Instinct candidates ({} from last 7 days of task outcomes):\n", candidates.len());
    for (i, c) in candidates.iter().enumerate() {
        println!("[{}] {:.0}% confidence", i + 1, c.confidence * 100.0);
        println!("    Rule:     {}", c.rule);
        println!("    Evidence: {}", c.evidence);
        if let Some(ref p) = c.project {
            println!("    Project:  {}", p);
        }
        println!();
    }

    if auto {
        let mut engine = InstinctEngine::init();
        for c in &candidates {
            engine.add_rule(c.rule.clone());
        }
        match engine.save() {
            Ok(()) => println!("✓ Added {} instinct(s) automatically.", candidates.len()),
            Err(e) => eprintln!("Save failed: {e}"),
        }
        return;
    }

    print!("Accept? (y=all / 1,2,3=specific / n=none): ");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    let _ = std::io::stdin().read_line(&mut input);
    let input = input.trim().to_lowercase();

    let accepted: Vec<&crate::instinct::InstinctCandidate> = if input == "y" {
        candidates.iter().collect()
    } else if input == "n" || input.is_empty() {
        vec![]
    } else {
        input
            .split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .filter(|&i| i >= 1 && i <= candidates.len())
            .map(|i| &candidates[i - 1])
            .collect()
    };

    if accepted.is_empty() {
        println!("No instincts added.");
        return;
    }

    let mut engine = InstinctEngine::init();
    for c in &accepted {
        engine.add_rule(c.rule.clone());
        println!("✓ Added: \"{}\"", c.rule);
    }
    match engine.save() {
        Ok(()) => println!("Saved {} instinct(s).", accepted.len()),
        Err(e) => eprintln!("Save failed: {e}"),
    }
}
```

- [ ] **Step 5.4: cargo build**

```bash
cargo build --bin raios 2>&1 | tail -5
```

- [ ] **Step 5.5: Smoke test**

```bash
cargo run --bin raios -- instinct review 2>&1
```

Beklenen: `No task outcomes in the last 7 days. Run some tasks first.`

- [ ] **Step 5.6: Tüm testleri çalıştır**

```bash
cargo test --lib 2>&1 | grep "test result"
```

- [ ] **Step 5.7: Final commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): raios instinct review — interactive evolutionary instinct approval"
```
