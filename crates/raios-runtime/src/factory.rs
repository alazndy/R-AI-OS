/// Factory Mode — async heavy-task queue with Job ID return and inbox notification.
///
/// Agents offload long-running tasks (refactoring, test generation, builds) by
/// submitting a `JobRequest`. R-AI-OS returns a `job_id` immediately, then
/// executes the task in a background tokio task. On completion the result is:
///   1. Persisted in SQLite (inbox table)
///   2. Broadcast over the shared channel as `{"event":"JobComplete", ...}`
///   3. Optionally POSTed to a `webhook_url` if provided
use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

// ─── Job types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            JobStatus::Pending => "pending",
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub description: String,
    pub agent: String,
    pub project: Option<String>,
    pub status: JobStatus,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub webhook_url: Option<String>,
}

impl Job {
    pub fn new(
        description: &str,
        agent: &str,
        project: Option<&str>,
        webhook_url: Option<&str>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            description: description.to_string(),
            agent: agent.to_string(),
            project: project.map(ToOwned::to_owned),
            status: JobStatus::Pending,
            created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            completed_at: None,
            result: None,
            error: None,
            webhook_url: webhook_url.map(ToOwned::to_owned),
        }
    }
}

// ─── Task function type ───────────────────────────────────────────────────────

pub type TaskFn = Pin<Box<dyn Future<Output = Result<String>> + Send + 'static>>;

// ─── Factory ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Factory {
    db_path: Arc<PathBuf>,
    tx: broadcast::Sender<String>,
    #[allow(dead_code)]
    write_lock: Arc<Mutex<()>>,
}

impl Factory {
    pub fn new(tx: broadcast::Sender<String>) -> Self {
        let db_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios")
            .join("workspace.db");

        let factory = Self {
            db_path: Arc::new(db_path),
            tx,
            write_lock: Arc::new(Mutex::new(())),
        };

        // Ensure table exists
        factory.ensure_table();
        factory
    }

    fn connect(&self) -> Result<Connection> {
        let conn = Connection::open(self.db_path.as_ref())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(conn)
    }

    fn ensure_table(&self) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS factory_jobs (
                    id           TEXT PRIMARY KEY,
                    description  TEXT NOT NULL,
                    agent        TEXT NOT NULL,
                    project      TEXT,
                    status       TEXT NOT NULL DEFAULT 'pending',
                    created_at   TEXT NOT NULL,
                    completed_at TEXT,
                    result       TEXT,
                    error        TEXT,
                    webhook_url  TEXT
                );",
            );
        }
    }

    /// Submit a job and return the job ID immediately.
    /// The task runs asynchronously in a spawned tokio task.
    pub fn submit(&self, job: Job, task: TaskFn) -> Uuid {
        let id = job.id;
        self.persist(&job);

        let factory = self.clone();
        let job_id = id;

        tokio::spawn(async move {
            factory.set_running(&job_id);

            match task.await {
                Ok(result) => factory.complete(&job_id, &result),
                Err(e) => factory.fail(&job_id, &e.to_string()),
            }
        });

        id
    }

    fn persist(&self, job: &Job) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO factory_jobs
                 (id, description, agent, project, status, created_at, webhook_url)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    job.id.to_string(),
                    job.description,
                    job.agent,
                    job.project,
                    job.status.to_string(),
                    job.created_at,
                    job.webhook_url,
                ],
            );
        }
    }

    fn set_running(&self, id: &Uuid) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "UPDATE factory_jobs SET status='running' WHERE id=?1",
                params![id.to_string()],
            );
        }
        let msg = serde_json::json!({
            "event": "JobStarted",
            "job_id": id.to_string()
        });
        let _ = self.tx.send(msg.to_string());
    }

    fn complete(&self, id: &Uuid, result: &str) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "UPDATE factory_jobs SET status='completed', result=?2, completed_at=?3 WHERE id=?1",
                params![id.to_string(), result, now],
            );
        }

        let msg = serde_json::json!({
            "event": "JobComplete",
            "job_id": id.to_string(),
            "status": "completed",
            "result": result,
            "completed_at": now,
        });
        let _ = self.tx.send(msg.to_string());
        // Persist each output line to the log ring buffer
        if let Ok(conn) = raios_core::db::open_db() {
            let short_id = &id.to_string()[..8];
            for line in result.lines().filter(|l| !l.trim().is_empty()) {
                let _ = raios_core::db::cp_log_append(&conn, "RUN", line);
            }
            let _ = raios_core::db::cp_log_append(&conn, "RUN", &format!("✓ [{}] done", short_id));
        }

        // Fire webhook if configured
        if let Some(job) = self.get(id) {
            if let Some(url) = job.webhook_url {
                fire_webhook_detached(url, msg.clone());
            }
        }
    }

    fn fail(&self, id: &Uuid, error: &str) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "UPDATE factory_jobs SET status='failed', error=?2, completed_at=?3 WHERE id=?1",
                params![id.to_string(), error, now],
            );
        }

        let msg = serde_json::json!({
            "event": "JobFailed",
            "job_id": id.to_string(),
            "status": "failed",
            "error": error,
            "completed_at": now,
        });
        let _ = self.tx.send(msg.to_string());
        if let Ok(conn) = raios_core::db::open_db() {
            let short_id = &id.to_string()[..8];
            let _ = raios_core::db::cp_log_append(&conn, "RUN", &format!("✗ [{}] {}", short_id, error));
        }
    }

    pub fn get(&self, id: &Uuid) -> Option<Job> {
        let conn = self.connect().ok()?;
        conn.query_row(
            "SELECT id,description,agent,project,status,created_at,completed_at,result,error,webhook_url
             FROM factory_jobs WHERE id=?1",
            params![id.to_string()],
            row_to_job,
        )
        .ok()
    }

    pub fn list_inbox(&self, limit: usize) -> Vec<Job> {
        let Ok(conn) = self.connect() else {
            return vec![];
        };
        let mut stmt = conn
            .prepare(
                "SELECT id,description,agent,project,status,created_at,completed_at,result,error,webhook_url
                 FROM factory_jobs
                 WHERE status IN ('completed','failed')
                 ORDER BY completed_at DESC LIMIT ?1",
            )
            .ok();

        match &mut stmt {
            Some(s) => s
                .query_map(params![limit as i64], row_to_job)
                .ok()
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default(),
            None => vec![],
        }
    }

    pub fn list_running(&self) -> Vec<Job> {
        let Ok(conn) = self.connect() else {
            return vec![];
        };
        let mut stmt = conn
            .prepare(
                "SELECT id,description,agent,project,status,created_at,completed_at,result,error,webhook_url
                 FROM factory_jobs WHERE status IN ('pending','running') ORDER BY created_at ASC",
            )
            .ok();

        match &mut stmt {
            Some(s) => s
                .query_map(params![], row_to_job)
                .ok()
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default(),
            None => vec![],
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn row_to_job(row: &rusqlite::Row) -> rusqlite::Result<Job> {
    let status_str: String = row.get(4)?;
    let status = match status_str.as_str() {
        "running" => JobStatus::Running,
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        _ => JobStatus::Pending,
    };
    let id_str: String = row.get(0)?;
    Ok(Job {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
        description: row.get(1)?,
        agent: row.get(2)?,
        project: row.get(3)?,
        status,
        created_at: row.get(5)?,
        completed_at: row.get(6)?,
        result: row.get(7)?,
        error: row.get(8)?,
        webhook_url: row.get(9)?,
    })
}

fn fire_webhook_detached(url: String, payload: serde_json::Value) {
    // Fire-and-forget: spawn a blocking thread so we don't need reqwest
    std::thread::spawn(move || {
        let body = payload.to_string();
        // Use curl if available (no extra dependency required)
        let _ = std::process::Command::new("curl")
            .args([
                "-s",
                "-X",
                "POST",
                "-H",
                "Content-Type: application/json",
                "-d",
                &body,
                &url,
            ])
            .output();
    });
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_factory(tmp: &TempDir) -> Factory {
        let db_path = tmp.path().join("test.db");
        let (tx, _) = broadcast::channel::<String>(16);
        let factory = Factory {
            db_path: Arc::new(db_path),
            tx,
            write_lock: Arc::new(Mutex::new(())),
        };
        factory.ensure_table();
        factory
    }

    #[tokio::test]
    async fn submit_returns_job_id_immediately() {
        let tmp = TempDir::new().unwrap();
        let factory = make_factory(&tmp);

        let job = Job::new("test task", "claude", Some("r-ai-os"), None);
        let id = factory.submit(job, Box::pin(async { Ok("done".to_string()) }));

        // Job ID is returned before the task completes
        assert!(!id.is_nil());
    }

    #[tokio::test]
    async fn completed_job_appears_in_inbox() {
        let tmp = TempDir::new().unwrap();
        let factory = make_factory(&tmp);

        let job = Job::new("inbox test", "claude", None, None);
        let id = factory.submit(
            job,
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                Ok("result text".to_string())
            }),
        );

        // Poll instead of a single fixed sleep — a flat 100ms was flaky under
        // slower/loaded CI runners (observed timing out on Windows).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let completed = loop {
            let inbox = factory.list_inbox(10);
            if let Some(job) = inbox.iter().find(|j| j.id == id && j.status == JobStatus::Completed) {
                break job.clone();
            }
            assert!(std::time::Instant::now() < deadline, "job {id} did not complete in time");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        assert_eq!(completed.result.as_deref(), Some("result text"));
    }

    #[tokio::test]
    async fn failed_job_recorded_with_error() {
        let tmp = TempDir::new().unwrap();
        let factory = make_factory(&tmp);

        let job = Job::new("failing task", "codex", None, None);
        let id = factory.submit(
            job,
            Box::pin(async { Err(anyhow::anyhow!("something broke")) }),
        );

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let job = factory.get(&id).unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert!(job
            .error
            .as_deref()
            .unwrap_or("")
            .contains("something broke"));
    }

    #[tokio::test]
    async fn shell_job_captures_output() {
        let tmp = TempDir::new().unwrap();
        let (tx, _) = broadcast::channel::<String>(16);
        let factory = Factory {
            db_path: Arc::new(tmp.path().join("t.db")),
            tx,
            write_lock: Arc::new(Mutex::new(())),
        };
        factory.ensure_table();

        let job = Job::new("echo test", "claude", None, None);
        let id = factory.submit(job, Box::pin(async { Ok("hello from shell".to_string()) }));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let j = factory.get(&id).unwrap();
        assert_eq!(j.status, JobStatus::Completed);
        assert!(j.result.as_deref().unwrap_or("").contains("hello"));
    }

    #[tokio::test]
    async fn broadcast_fires_on_completion() {
        let tmp = TempDir::new().unwrap();
        let (tx, mut rx) = broadcast::channel::<String>(16);
        let factory = Factory {
            db_path: Arc::new(tmp.path().join("t.db")),
            tx,
            write_lock: Arc::new(Mutex::new(())),
        };
        factory.ensure_table();

        let job = Job::new("broadcast test", "claude", None, None);
        factory.submit(job, Box::pin(async { Ok("ok".to_string()) }));

        // Expect JobStarted and JobComplete
        let mut events: Vec<String> = vec![];
        // 500ms was flaky under loaded/slower CI runners (observed timing out on Windows).
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            tokio::select! {
                Ok(msg) = rx.recv() => {
                    events.push(msg.clone());
                    if msg.contains("JobComplete") { break; }
                }
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }
        assert!(events.iter().any(|e| e.contains("JobComplete")));
    }
}
