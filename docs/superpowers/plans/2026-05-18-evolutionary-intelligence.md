# Plan 4: Evolutionary Intelligence — Autonomous Instinct Refinement

> **Subagent-Driven Development** ile execute et. HKUDS/OpenSpace'den ilham alındı.

**Goal:** Ajanlar task tamamladıktan sonra neyin işe yarayıp neyin yaramadığını otomatik olarak öğrenir ve instinct'lerini günceller. Manuel `raios instinct add` olmadan sistem kendi kendine evrilir.

**Philosophy:** "Her başarılı task bir öğrenme fırsatıdır. Her başarısız task bir uyarıdır."

**Architecture:** `TaskOutcomeLogger` → task tamamlandıktan sonra sonucu kaydeder. `InstinctRefinementWorker` (`aiosd`) → birikmiş sonuçları analiz eder, yeni instinct'ler önerir. Kullanıcı onayıyla (veya auto-approve modunda) global instinct'lere eklenir.

---

## Öğrenme Döngüsü

```
Task Dispatch
    ↓
TaskExecution (agent runs)
    ↓
TaskOutcome { success, duration, error_type?, files_changed }
    ↓
OutcomeStore (SQLite: task_outcomes tablosu)
    ↓
InstinctRefinementWorker (aiosd, her 24 saatte bir)
    ├── Pattern detection: "Bu proje tipinde build %80 başarısız → instinct ekle"
    ├── Instinct candidate generation
    └── auto_approve veya user_review modunda kaydet
```

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/db.rs` | `task_outcomes` tablosu + CRUD |
| `src/tasks.rs` | `TaskOutcome` logging |
| `src/instinct.rs` | `refine_instincts()` + pattern detectors |
| `src/daemon/refinement.rs` | Yeni — `InstinctRefinementWorker` |
| `src/daemon/mod.rs` | `pub mod refinement;` |
| `src/daemon/server.rs` | Refinement RPC + broadcast |
| `src/cli.rs` | `raios instinct review` — pending refinements |

---

## Task 1: `task_outcomes` DB tablosu

**Files:** `src/db.rs`

- [ ] `migrate()`'e ekle:
```rust
conn.execute_batch("
    CREATE TABLE IF NOT EXISTS task_outcomes (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        task_id     TEXT NOT NULL,
        project     TEXT NOT NULL,
        agent       TEXT NOT NULL,
        description TEXT NOT NULL,
        success     INTEGER NOT NULL,  -- 0 or 1
        duration_ms INTEGER,
        error_type  TEXT,              -- 'build_fail'|'test_fail'|'timeout'|null
        files_changed INTEGER DEFAULT 0,
        created_at  TEXT NOT NULL DEFAULT (datetime('now'))
    )
")?;
```

- [ ] Public API:
```rust
pub fn log_task_outcome(conn: &Connection, outcome: &TaskOutcome) -> Result<()>
pub fn get_outcomes_since(conn: &Connection, hours: i64) -> Result<Vec<TaskOutcome>>
pub fn get_project_outcomes(conn: &Connection, project: &str, limit: i64) -> Result<Vec<TaskOutcome>>
```

- [ ] `TaskOutcome` struct:
```rust
pub struct TaskOutcome {
    pub task_id: String,
    pub project: String,
    pub agent: String,
    pub description: String,
    pub success: bool,
    pub duration_ms: Option<i64>,
    pub error_type: Option<String>,
    pub files_changed: usize,
}
```

- [ ] `cargo check` → temiz

- [ ] Commit: `feat(db): task_outcomes table for evolutionary intelligence`

---

## Task 2: Task outcome logging

**Files:** `src/tasks.rs`

- [ ] `dispatch_to_agent()` tamamlandıktan sonra sonucu logla:
```rust
// Mevcut dispatch sonrasına ekle:
let outcome = TaskOutcome {
    task_id: task.id.to_string(),
    project: task.project.clone(),
    agent: task.agent.clone(),
    description: task.description.clone(),
    success: result.ok,
    duration_ms: Some(result.duration_ms as i64),
    error_type: result.error_type.clone(),
    files_changed: result.files_changed,
};
if let Ok(conn) = crate::db::open_db() {
    let _ = crate::db::log_task_outcome(&conn, &outcome);
}
```

- [ ] `cargo check` → temiz

- [ ] Commit: `feat(tasks): log task outcomes to DB for instinct refinement`

---

## Task 3: Pattern Detectors + `refine_instincts()`

**Files:** `src/instinct.rs`

- [ ] Pattern detector fonksiyonları ekle:
```rust
pub struct InstinctCandidate {
    pub rule: String,
    pub confidence: f32,  // 0.0-1.0
    pub evidence: String, // "3/5 build tasks failed in last 7 days"
    pub project: Option<String>, // None = global
}

pub fn refine_instincts(outcomes: &[TaskOutcome]) -> Vec<InstinctCandidate> {
    let mut candidates = Vec::new();

    // Pattern 1: High build failure rate
    let build_tasks: Vec<_> = outcomes.iter()
        .filter(|o| o.error_type.as_deref() == Some("build_fail"))
        .collect();
    if build_tasks.len() >= 3 {
        let fail_rate = build_tasks.iter().filter(|o| !o.success).count() as f32
            / build_tasks.len() as f32;
        if fail_rate > 0.6 {
            candidates.push(InstinctCandidate {
                rule: "High build failure rate — run `cargo check` before dispatching build tasks".into(),
                confidence: fail_rate,
                evidence: format!("{}/{} build tasks failed", build_tasks.iter().filter(|o| !o.success).count(), build_tasks.len()),
                project: build_tasks[0].project.clone().into(),
            });
        }
    }

    // Pattern 2: Slow agents
    let slow: Vec<_> = outcomes.iter()
        .filter(|o| o.duration_ms.unwrap_or(0) > 300_000) // >5min
        .collect();
    if slow.len() >= 2 {
        candidates.push(InstinctCandidate {
            rule: format!("Tasks frequently timeout ({}+ occurrences) — break into smaller subtasks", slow.len()),
            confidence: 0.8,
            evidence: format!("{} tasks exceeded 5 minutes", slow.len()),
            project: None,
        });
    }

    // Pattern 3: High success with specific agent
    for agent in &["claude", "gemini", "codex"] {
        let agent_tasks: Vec<_> = outcomes.iter().filter(|o| o.agent == *agent).collect();
        if agent_tasks.len() >= 5 {
            let success_rate = agent_tasks.iter().filter(|o| o.success).count() as f32
                / agent_tasks.len() as f32;
            if success_rate > 0.9 {
                candidates.push(InstinctCandidate {
                    rule: format!("{} has {:.0}% success rate on this project — prefer it for complex tasks", agent, success_rate * 100.0),
                    confidence: success_rate,
                    evidence: format!("{}/{} tasks succeeded", agent_tasks.iter().filter(|o| o.success).count(), agent_tasks.len()),
                    project: agent_tasks[0].project.clone().into(),
                });
            }
        }
    }

    candidates
}
```

- [ ] Unit tests: 3 test (build fail pattern, slow agent pattern, agent preference)

- [ ] Commit: `feat(instinct): pattern detectors + refine_instincts() for evolutionary learning`

---

## Task 4: `InstinctRefinementWorker` in aiosd

**Files:** `src/daemon/refinement.rs`

- [ ] Oluştur:
```rust
use crate::instinct::{refine_instincts, InstinctCandidate, InstinctEngine};
use tokio::time::{interval, Duration};

pub async fn start_refinement_worker(
    tx: tokio::sync::broadcast::Sender<String>,
) {
    let mut ticker = interval(Duration::from_secs(86400)); // daily
    loop {
        ticker.tick().await;
        run_refinement_cycle(&tx).await;
    }
}

async fn run_refinement_cycle(tx: &tokio::sync::broadcast::Sender<String>) {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };

    let outcomes = match crate::db::get_outcomes_since(&conn, 168) { // 7 days
        Ok(o) => o,
        Err(_) => return,
    };

    if outcomes.is_empty() { return; }

    let candidates = refine_instincts(&outcomes);
    if candidates.is_empty() { return; }

    // Auto-approve high-confidence candidates (>0.85)
    let mut engine = InstinctEngine::init();
    let mut auto_added = Vec::new();
    let mut pending = Vec::new();

    for c in candidates {
        if c.confidence > 0.85 {
            engine.add_rule(c.rule.clone());
            auto_added.push(c);
        } else {
            pending.push(c);
        }
    }

    let _ = engine.save();

    // Broadcast to VS Code / TUI
    let event = serde_json::json!({
        "event": "InstinctRefinement",
        "auto_added": auto_added.len(),
        "pending_review": pending.len(),
        "candidates": pending.iter().map(|c| &c.rule).collect::<Vec<_>>()
    });
    let _ = tx.send(event.to_string());
}
```

- [ ] `src/daemon/mod.rs`'e `pub mod refinement;` ekle

- [ ] `src/daemon/server.rs`'de worker spawn:
```rust
let refinement_tx = tx.clone();
tokio::spawn(async move {
    crate::daemon::refinement::start_refinement_worker(refinement_tx).await;
});
```

- [ ] Commit: `feat(daemon): InstinctRefinementWorker — daily autonomous instinct learning`

---

## Task 5: `raios instinct review` CLI

**Files:** `src/cli.rs`

- [ ] `InstinctCmd`'e `Review` varyantı ekle:
```rust
/// Review pending instinct candidates from evolutionary learning
Review,
```

- [ ] `cmd_instinct_review()`:
```rust
fn cmd_instinct_review(_json: bool) {
    // Read pending candidates from a temp store (or broadcast cache)
    // Show list with [y/n] per candidate
    println!("Pending instinct candidates:");
    println!("  (Run `raios instinct suggest` for project-specific suggestions)");
    // TODO: read from DB pending_instincts table (Phase 2)
}
```

- [ ] Commit: `feat(cli): raios instinct review — pending evolutionary candidates`
