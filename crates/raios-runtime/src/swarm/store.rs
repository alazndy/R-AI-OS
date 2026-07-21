use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::swarm::{SwarmStatus, SwarmTask};

pub struct SwarmStore {
    db_path: PathBuf,
}

impl SwarmStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        let db_path = db_path.into();
        let s = Self { db_path };
        // connect() runs raios_core::db::migrate_existing(), the single
        // source of truth for this table's schema (see schema.rs) — used
        // to duplicate the CREATE TABLE + a bolt-on ALTER TABLE here, which
        // had drifted out of sync with the central migration.
        let _ = s.connect();
        s
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios")
            .join("workspace.db")
    }

    fn connect(&self) -> Result<Connection> {
        if let Some(p) = self.db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        raios_core::db::migrate_existing(&conn)?;
        Ok(conn)
    }

    pub fn create(
        &self,
        project_name: &str,
        project_path: &Path,
        description: &str,
        agent: &str,
    ) -> Result<SwarmTask> {
        let id = Uuid::new_v4();
        let (worktree_path, branch_name) =
            crate::swarm::worktree::create_worktree(project_path, id, description)?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let conn = self.connect()?;
        let workflow_ids = raios_core::db::create_swarm_workflow(
            &conn,
            project_path.to_str().unwrap_or(""),
            description,
            agent,
        )?;
        conn.execute(
            "INSERT INTO swarm_tasks
             (id,project_name,project_path,worktree_path,branch_name,description,agent,created_at,cp_task_id,cp_agent_run_id,cp_artifact_id,cp_approval_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL,NULL)",
            params![
                id.to_string(),
                project_name,
                project_path.to_str().unwrap_or(""),
                worktree_path.to_str().unwrap_or(""),
                branch_name,
                description,
                agent,
                now,
                workflow_ids.task_id,
                workflow_ids.agent_run_id
            ],
        )?;
        let _ = raios_core::db::mark_swarm_workflow_running(
            &conn,
            &workflow_ids.task_id,
            &workflow_ids.agent_run_id,
        );

        Ok(SwarmTask {
            id,
            project_name: project_name.to_string(),
            project_path: project_path.to_path_buf(),
            worktree_path,
            branch_name,
            task_description: description.to_string(),
            agent: agent.to_string(),
            status: SwarmStatus::Initializing,
            created_at: now,
            task_id: Some(workflow_ids.task_id),
            agent_run_id: Some(workflow_ids.agent_run_id),
            artifact_id: None,
            approval_id: None,
        })
    }

    pub fn get(&self, id: &str) -> Option<SwarmTask> {
        let conn = self.connect().ok()?;
        conn.query_row(
            "SELECT id,project_name,project_path,worktree_path,branch_name,
                    description,agent,status,created_at,cp_task_id,cp_agent_run_id,cp_artifact_id,cp_approval_id
             FROM swarm_tasks WHERE id=?1",
            params![id],
            |row| {
                let status_str: String = row.get(7)?;
                Ok(SwarmTask {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                    project_name: row.get(1)?,
                    project_path: PathBuf::from(row.get::<_, String>(2)?),
                    worktree_path: PathBuf::from(row.get::<_, String>(3)?),
                    branch_name: row.get(4)?,
                    task_description: row.get(5)?,
                    agent: row.get(6)?,
                    status: parse_status(&status_str),
                    created_at: row.get(8)?,
                    task_id: row.get(9)?,
                    agent_run_id: row.get(10)?,
                    artifact_id: row.get(11)?,
                    approval_id: row.get(12)?,
                })
            },
        )
        .ok()
    }

    pub fn list_active(&self) -> Vec<SwarmTask> {
        let Ok(conn) = self.connect() else {
            return vec![];
        };
        let mut stmt = conn
            .prepare(
                "SELECT id,project_name,project_path,worktree_path,branch_name,
                        description,agent,status,created_at,cp_task_id,cp_agent_run_id,cp_artifact_id,cp_approval_id
                 FROM swarm_tasks WHERE status NOT IN ('merged','rejected','failed')",
            )
            .ok();
        match &mut stmt {
            Some(s) => s
                .query_map([], |row| {
                    let status_str: String = row.get(7)?;
                    Ok(SwarmTask {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?)
                            .unwrap_or_else(|_| Uuid::nil()),
                        project_name: row.get(1)?,
                        project_path: PathBuf::from(row.get::<_, String>(2)?),
                        worktree_path: PathBuf::from(row.get::<_, String>(3)?),
                        branch_name: row.get(4)?,
                        task_description: row.get(5)?,
                        agent: row.get(6)?,
                        status: parse_status(&status_str),
                        created_at: row.get(8)?,
                        task_id: row.get(9)?,
                        agent_run_id: row.get(10)?,
                        artifact_id: row.get(11)?,
                        approval_id: row.get(12)?,
                    })
                })
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default(),
            None => vec![],
        }
    }

    pub fn set_status(&self, id: &str, status: SwarmStatus) {
        let Ok(conn) = self.connect() else { return };
        let ids = conn
            .query_row(
                "SELECT cp_task_id, cp_agent_run_id, cp_artifact_id, cp_approval_id, project_path, branch_name, worktree_path
                 FROM swarm_tasks WHERE id=?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .ok();
        let s = status_str(&status);
        let completed = matches!(
            status,
            SwarmStatus::Merged | SwarmStatus::Rejected | SwarmStatus::Failed(_)
        );
        if let Some((
            Some(task_id),
            Some(agent_run_id),
            artifact_id,
            approval_id,
            project_path,
            branch_name,
            worktree_path,
        )) = ids
        {
            match &status {
                SwarmStatus::Initializing => {}
                SwarmStatus::Running => {
                    let _ =
                        raios_core::db::mark_swarm_workflow_running(&conn, &task_id, &agent_run_id);
                }
                SwarmStatus::AwaitingReview => {
                    if let Ok((artifact_id_new, approval_id_new)) =
                        raios_core::db::ensure_swarm_review_artifacts(
                            &conn,
                            &task_id,
                            &agent_run_id,
                            raios_core::db::project_id_for_file_path(&conn, &project_path),
                            &project_path,
                            &branch_name,
                            &worktree_path,
                        )
                    {
                        let _ = conn.execute(
                            "UPDATE swarm_tasks SET cp_artifact_id=?2, cp_approval_id=?3 WHERE id=?1",
                            params![id, artifact_id_new, approval_id_new],
                        );
                    }
                }
                SwarmStatus::Merged => {
                    let _ = raios_core::db::mark_swarm_workflow_merged(
                        &conn,
                        &task_id,
                        &agent_run_id,
                        artifact_id.as_deref(),
                        approval_id.as_deref(),
                    );
                }
                SwarmStatus::Rejected => {
                    let _ = raios_core::db::mark_swarm_workflow_rejected(
                        &conn,
                        &task_id,
                        &agent_run_id,
                        artifact_id.as_deref(),
                        approval_id.as_deref(),
                    );
                }
                SwarmStatus::Failed(reason) => {
                    let _ = raios_core::db::mark_swarm_workflow_rejected(
                        &conn,
                        &task_id,
                        &agent_run_id,
                        artifact_id.as_deref(),
                        approval_id.as_deref(),
                    );
                    let _ = conn.execute(
                        "UPDATE cp_agent_runs SET exit_reason=?2 WHERE id=?1",
                        params![agent_run_id, reason],
                    );
                }
            }
        }
        if completed {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let _ = conn.execute(
                "UPDATE swarm_tasks SET status=?2, completed_at=?3 WHERE id=?1",
                params![id, s, now],
            );
        } else {
            let _ = conn.execute(
                "UPDATE swarm_tasks SET status=?2 WHERE id=?1",
                params![id, s],
            );
        }
    }
}

fn status_str(s: &SwarmStatus) -> &'static str {
    match s {
        SwarmStatus::Initializing => "initializing",
        SwarmStatus::Running => "running",
        SwarmStatus::AwaitingReview => "awaiting_review",
        SwarmStatus::Merged => "merged",
        SwarmStatus::Rejected => "rejected",
        SwarmStatus::Failed(_) => "failed",
    }
}

fn parse_status(s: &str) -> SwarmStatus {
    match s {
        "running" => SwarmStatus::Running,
        "awaiting_review" => SwarmStatus::AwaitingReview,
        "merged" => SwarmStatus::Merged,
        "rejected" => SwarmStatus::Rejected,
        "failed" => SwarmStatus::Failed("unknown".into()),
        _ => SwarmStatus::Initializing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store(tmp: &tempfile::TempDir) -> SwarmStore {
        SwarmStore::new(tmp.path().join("test.db"))
    }

    fn fake_task(
        store: &SwarmStore,
        tmp: &tempfile::TempDir,
        desc: &str,
        agent: &str,
    ) -> SwarmTask {
        let id = Uuid::new_v4();
        let branch = format!("swarm/{}-test", &id.to_string()[..8]);
        let worktree = tmp.path().join("wt").join(id.to_string());
        let now = "2026-01-01 00:00:00".to_string();
        let conn = store.connect().unwrap();
        let workflow =
            raios_core::db::create_swarm_workflow(&conn, "/tmp/proj", desc, agent).unwrap();
        conn.execute(
            "INSERT INTO swarm_tasks
             (id,project_name,project_path,worktree_path,branch_name,description,agent,created_at,cp_task_id,cp_agent_run_id,cp_artifact_id,cp_approval_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL,NULL)",
            params![
                id.to_string(),
                "test-project",
                "/tmp/proj",
                worktree.to_str().unwrap(),
                branch,
                desc,
                agent,
                now,
                workflow.task_id,
                workflow.agent_run_id
            ],
        )
        .unwrap();
        SwarmTask {
            id,
            project_name: "test-project".into(),
            project_path: PathBuf::from("/tmp/proj"),
            worktree_path: worktree,
            branch_name: format!("swarm/{}-test", &id.to_string()[..8]),
            task_description: desc.to_string(),
            agent: agent.to_string(),
            status: SwarmStatus::Initializing,
            created_at: now,
            task_id: Some(workflow.task_id),
            agent_run_id: Some(workflow.agent_run_id),
            artifact_id: None,
            approval_id: None,
        }
    }

    #[test]
    fn insert_and_get_swarm_task() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let task = fake_task(&store, &tmp, "add dark mode", "claude");
        let fetched = store.get(&task.id.to_string()).unwrap();
        assert_eq!(fetched.task_description, "add dark mode");
        assert_eq!(fetched.agent, "claude");
    }

    #[test]
    fn list_active_swarm_tasks() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        fake_task(&store, &tmp, "task a", "claude");
        fake_task(&store, &tmp, "task b", "claude");
        assert_eq!(store.list_active().len(), 2);
    }

    #[test]
    fn update_status_changes_task() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let task = fake_task(&store, &tmp, "refactor auth", "claude");
        store.set_status(&task.id.to_string(), SwarmStatus::AwaitingReview);
        let fetched = store.get(&task.id.to_string()).unwrap();
        assert_eq!(fetched.status, SwarmStatus::AwaitingReview);
    }

    #[test]
    fn completed_status_sets_completed_at() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let task = fake_task(&store, &tmp, "add feature", "claude");
        store.set_status(&task.id.to_string(), SwarmStatus::Merged);
        let conn = store.connect().unwrap();
        let completed_at: Option<String> = conn
            .query_row(
                "SELECT completed_at FROM swarm_tasks WHERE id=?1",
                params![task.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(completed_at.is_some());
    }

    #[test]
    fn merged_task_excluded_from_active_list() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let t1 = fake_task(&store, &tmp, "active task", "claude");
        let t2 = fake_task(&store, &tmp, "merged task", "claude");
        store.set_status(&t2.id.to_string(), SwarmStatus::Merged);
        let active = store.list_active();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, t1.id);
    }

    #[test]
    fn awaiting_review_creates_control_plane_artifacts() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let task = fake_task(&store, &tmp, "review auth refactor", "claude");
        store.set_status(&task.id.to_string(), SwarmStatus::AwaitingReview);

        let conn = store.connect().unwrap();
        let artifact_id: Option<String> = conn
            .query_row(
                "SELECT cp_artifact_id FROM swarm_tasks WHERE id=?1",
                params![task.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        let approval_id: Option<String> = conn
            .query_row(
                "SELECT cp_approval_id FROM swarm_tasks WHERE id=?1",
                params![task.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        let task_status: String = conn
            .query_row(
                "SELECT status FROM cp_tasks WHERE id=?1",
                params![task.task_id.clone().unwrap()],
                |r| r.get(0),
            )
            .unwrap();

        assert!(artifact_id.is_some());
        assert!(approval_id.is_some());
        assert_eq!(task_status, "awaiting_approval");
    }

    #[test]
    fn merged_swarm_updates_control_plane_task() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let task = fake_task(&store, &tmp, "merge auth refactor", "claude");
        store.set_status(&task.id.to_string(), SwarmStatus::AwaitingReview);
        store.set_status(&task.id.to_string(), SwarmStatus::Merged);

        let conn = store.connect().unwrap();
        let task_status: String = conn
            .query_row(
                "SELECT status FROM cp_tasks WHERE id=?1",
                params![task.task_id.clone().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        let run_status: String = conn
            .query_row(
                "SELECT status FROM cp_agent_runs WHERE id=?1",
                params![task.agent_run_id.clone().unwrap()],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(task_status, "completed");
        assert_eq!(run_status, "succeeded");
    }
}
