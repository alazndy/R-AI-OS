use super::*;
use rusqlite::{params, Connection, Result, Transaction, TransactionBehavior};
pub fn cp_session_start(
    conn: &Connection,
    agent_identity: &str,
    project_id: Option<i64>,
) -> Result<(String, String)> {
    let workspace_root = match project_id {
        Some(id) => conn.query_row("SELECT path FROM projects WHERE id=?1", [id], |r| {
            r.get::<_, String>(0)
        })?,
        None => std::env::current_dir()
            .map_err(|e| invalid_wrapper_note(&e.to_string()))?
            .to_string_lossy()
            .into_owned(),
    };
    cp_session_start_in_workspace(conn, agent_identity, project_id, &workspace_root, None)
}

/// Open task, contract, run, and optional process witness atomically.
pub fn cp_session_start_in_workspace(
    conn: &Connection,
    agent_identity: &str,
    project_id: Option<i64>,
    workspace_root: &str,
    owner: Option<&WrapperProcessIdentity>,
) -> Result<(String, String)> {
    cp_session_start_named(
        conn,
        agent_identity,
        project_id,
        workspace_root,
        owner,
        None,
        None,
    )
}

/// Store explicit session labels on the canonical task in the launch transaction.
pub fn cp_session_start_named(
    conn: &Connection,
    agent_identity: &str,
    project_id: Option<i64>,
    workspace_root: &str,
    owner: Option<&WrapperProcessIdentity>,
    name: Option<&str>,
    work: Option<&str>,
) -> Result<(String, String)> {
    validate_session_labels(name, work)?;
    if !std::path::Path::new(workspace_root).is_absolute() {
        return Err(invalid_wrapper_note("session workspace must be absolute"));
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    if let Some(id) = project_id {
        let path: String =
            tx.query_row("SELECT path FROM projects WHERE id=?1", [id], |r| r.get(0))?;
        let registered = std::path::Path::new(&path);
        let effective = std::path::Path::new(workspace_root);
        let registered = registered
            .canonicalize()
            .unwrap_or_else(|_| registered.to_path_buf());
        let effective = effective
            .canonicalize()
            .unwrap_or_else(|_| effective.to_path_buf());
        if !effective.starts_with(&registered) {
            return Err(invalid_wrapper_note(
                "session workspace does not belong to its project",
            ));
        }
    }
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let task_id = uuid::Uuid::new_v4().to_string();
    let run_id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_tasks
            (id, project_id, plan_id, parent_task_id, title, description, priority, status,
             assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, ?2, NULL, NULL, ?3, ?4, 50, 'in_progress',
             'agent', ?5, 'interactive wrapper session', ?6, ?6)",
        params![
            task_id,
            project_id,
            name.map(str::trim)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Session: {}", agent_identity)),
            work.map(str::trim)
                .map(str::to_owned)
                .unwrap_or_else(|| format!(
                    "Wrapper-routed interactive session for {}",
                    agent_identity
                )),
            agent_identity,
            now
        ],
    )?;

    let run_contract_id = cp_insert_run_contract(
        &tx,
        Some(&task_id),
        workspace_root,
        "[]",
        "[]",
        "[]",
        None,
        None,
    )?;

    tx.execute(
        "INSERT INTO cp_agent_runs
            (id, task_id, project_id, provider, agent_name, run_contract_id, attempt, status, started_at)
         VALUES (?1, ?2, ?3, 'wrapper', ?4, ?5, 1, 'running', ?6)",
        params![run_id, task_id, project_id, agent_identity, run_contract_id, now],
    )?;

    tx.execute(
        "INSERT INTO cp_wrapper_resume(agent_run_id,workspace_root) VALUES (?1,?2)",
        params![run_id, workspace_root],
    )?;

    if let Some(owner) = owner {
        let owner_json =
            serde_json::to_string(owner).map_err(|e| invalid_wrapper_note(&e.to_string()))?;
        tx.execute(
            "INSERT INTO cp_wrapper_processes(agent_run_id, owner_json) VALUES (?1,?2)",
            params![run_id, owner_json],
        )?;
    }
    tx.commit()?;
    Ok((task_id, run_id))
}

/// Close a wrapper session opened with cp_session_start.
pub fn cp_session_end(conn: &Connection, task_id: &str, run_id: &str, success: bool) -> Result<()> {
    cp_session_end_with_summary(conn, task_id, run_id, success, None)
}

pub fn cp_session_end_with_summary(
    conn: &Connection,
    task_id: &str,
    run_id: &str,
    success: bool,
    summary: Option<&str>,
) -> Result<()> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    cp_finish_wrapper_session(
        &tx,
        task_id,
        run_id,
        success,
        summary,
        if success {
            "clean_exit"
        } else {
            "nonzero_exit"
        },
    )?;
    tx.commit()
}

/// Finish a canonical running wrapper inside an existing transaction. Terminal
/// runs are idempotent; mismatched task/run pairs never update either row.
pub fn cp_finish_wrapper_session(
    conn: &Transaction<'_>,
    task_id: &str,
    run_id: &str,
    success: bool,
    summary: Option<&str>,
    exit_reason: &str,
) -> Result<bool> {
    let (run_task, provider, status): (String, String, String) = conn.query_row(
        "SELECT r.task_id,r.provider,r.status FROM cp_agent_runs r JOIN cp_tasks t ON t.id=r.task_id WHERE r.id=?1",
        [run_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if run_task != task_id || provider != "wrapper" {
        return Err(invalid_wrapper_note("session run/task ownership mismatch"));
    }
    if status != "running" {
        return Ok(false);
    }
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let run_status = if success { "succeeded" } else { "failed" };
    let task_status = if success { "completed" } else { "failed" };

    conn.execute(
        "UPDATE cp_agent_runs
         SET status=?1, ended_at=?2, exit_reason=?3, summary=COALESCE(?4, summary)
         WHERE id=?5",
        params![run_status, now, exit_reason, summary, run_id],
    )?;

    crate::security::record_audit_event(conn, "wrapper_session_finished", "raios_wrapper", &serde_json::json!({"run_id":run_id,"task_id":task_id,"success":success,"reason":exit_reason}).to_string())
        .map_err(|e| invalid_wrapper_note(&e.to_string()))?;
    conn.execute(
        "UPDATE cp_tasks SET status=?1, updated_at=?2 WHERE id=?3 AND status='in_progress'",
        params![task_status, now, task_id],
    )?;

    if success {
        if let Some(text) = summary.filter(|s| !s.is_empty()) {
            let event_summary = format!("agent run completed: {text}");
            // Best-effort: a missing notification-log row must never fail
            // the actual session-completion write, which is this
            // function's real job.
            let _ = crate::db::log_activity_event(
                conn,
                "agent_run",
                None,
                "routine",
                &event_summary,
                None,
            );
        }
    }

    Ok(true)
}

/// Process identity includes host, boot, PID namespace and start time: PID alone
/// cannot distinguish reuse or an inaccessible process namespace.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WrapperProcessIdentity {
    pub host_id: String,
    pub boot_id: String,
    pub pid_namespace: String,
    pub pid: u32,
    pub start_ticks: u64,
}

pub fn cp_attach_wrapper_child(
    conn: &Connection,
    run_id: &str,
    owner: &WrapperProcessIdentity,
    child: Option<&WrapperProcessIdentity>,
) -> Result<()> {
    let owner_json =
        serde_json::to_string(owner).map_err(|e| invalid_wrapper_note(&e.to_string()))?;
    let child_json = child
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| invalid_wrapper_note(&e.to_string()))?;
    let changed = conn.execute("UPDATE cp_wrapper_processes SET child_json=?1,child_attached=1 WHERE agent_run_id=?2 AND owner_json=?3 AND child_attached=0 AND EXISTS(SELECT 1 FROM cp_agent_runs WHERE id=?2 AND provider='wrapper' AND status='running')", params![child_json,run_id,owner_json])?;
    if changed != 1 {
        return Err(invalid_wrapper_note(
            "wrapper child is not owned by this active session",
        ));
    }
    Ok(())
}

/// List recent wrapper sessions (most recent first).
pub struct SessionRow {
    pub native_session_id: Option<String>,
    pub workspace_root: Option<String>,
    pub task_id: String,
    pub name: String,
    pub work: String,
    pub task_status: String,
    pub project_name: Option<String>,
    pub project_path: Option<String>,
    pub run_id: String,
    pub agent_name: String,
    pub status: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub exit_reason: Option<String>,
    pub summary: Option<String>,
}

/// A verified user note submitted by a live child of `raios run`.
///
/// The caller supplies only the opaque wrapper run ID, its current project
/// path, and the note. `cp_record_wrapper_memory_note` checks all ownership
/// boundaries against the control plane before persisting the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapperMemoryNote {
    pub event_id: String,
    pub agent_name: String,
    pub project_id: i64,
}

fn invalid_wrapper_note(reason: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(reason.to_string())
}

/// Store an explicit interactive note only for an open, project-bound wrapper
/// run. The run ID is an unguessable UUID passed only to the wrapper's child.
pub fn cp_record_wrapper_memory_note(
    conn: &Connection,
    run_id: &str,
    project_path: &str,
    note: &str,
) -> Result<WrapperMemoryNote> {
    let note = note.trim();
    if note.is_empty() || note.chars().count() > 500 {
        return Err(invalid_wrapper_note(
            "wrapper note must contain 1..=500 characters",
        ));
    }
    if raios_core::security::looks_like_secret(note).is_some() {
        return Err(invalid_wrapper_note("wrapper note resembles a secret"));
    }

    let project_id = project_id_for_file_path(conn, project_path)
        .ok_or_else(|| invalid_wrapper_note("wrapper note path is not a registered project"))?;
    let (agent_name, run_project_id): (String, Option<i64>) = conn
        .query_row(
            "SELECT agent_name, project_id
             FROM cp_agent_runs
             WHERE id=?1 AND provider='wrapper' AND status='running'",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| invalid_wrapper_note("wrapper run is not active"))?;
    if run_project_id != Some(project_id) {
        return Err(invalid_wrapper_note(
            "wrapper note project does not match its run",
        ));
    }

    let event_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "INSERT INTO cp_wrapper_events (id, agent_run_id, project_id, event_kind, content, created_at)
         VALUES (?1, ?2, ?3, 'memory_note', ?4, ?5)",
        params![event_id, run_id, project_id, note, now],
    )?;

    Ok(WrapperMemoryNote {
        event_id,
        agent_name,
        project_id,
    })
}

/// Reject empty, oversized, terminal-control-bearing, or secret-like labels.
pub fn validate_session_labels(name: Option<&str>, work: Option<&str>) -> Result<()> {
    for (label, limit) in [(name, 120), (work, 2000)] {
        if let Some(value) = label {
            let value = value.trim();
            if value.is_empty()
                || value.chars().count() > limit
                || value.chars().any(char::is_control)
                || crate::security::looks_like_secret(value).is_some()
            {
                return Err(invalid_wrapper_note("invalid session label: empty, oversized, control characters, or secret-like content"));
            }
        }
    }
    Ok(())
}

/// Update labels only through a verified wrapper/task link; audit and labels commit together.
pub fn cp_label_wrapper_session(
    conn: &Connection,
    run_id: &str,
    name: Option<&str>,
    work: Option<&str>,
) -> Result<()> {
    validate_session_labels(name, work)?;
    if name.is_none() && work.is_none() {
        return Err(invalid_wrapper_note(
            "provide a session name or work description",
        ));
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let task_id: String = tx.query_row(
        "SELECT t.id FROM cp_agent_runs r JOIN cp_tasks t ON t.id=r.task_id WHERE r.id=?1 AND r.provider='wrapper'",
        [run_id], |r| r.get(0))?;
    tx.execute("UPDATE cp_tasks SET title=COALESCE(?1,title),description=COALESCE(?2,description),updated_at=?3 WHERE id=?4",
        params![name.map(str::trim),work.map(str::trim),chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),task_id])?;
    crate::security::record_audit_event(&tx,"wrapper_session_labeled","local_user",
        &serde_json::json!({"run_id":run_id,"task_id":task_id,"name_changed":name.is_some(),"work_changed":work.is_some()}).to_string())
        .map_err(|e| invalid_wrapper_note(&e.to_string()))?;
    tx.commit()
}

pub fn cp_sessions_list(conn: &Connection, limit: usize) -> Result<Vec<SessionRow>> {
    cp_sessions_list_filtered(conn, limit, None, false, None, None)
}

/// Filters precede LIMIT so other agents' sessions cannot hide matching runs.
pub fn cp_sessions_list_filtered(
    conn: &Connection,
    limit: usize,
    agent: Option<&str>,
    active: bool,
    project: Option<&str>,
    search: Option<&str>,
) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT r.id,r.agent_name,r.status,r.started_at,r.ended_at,r.exit_reason,r.summary,
                t.id,t.title,t.description,t.status,p.name,p.path,j.native_session_id,j.workspace_root
         FROM cp_agent_runs r JOIN cp_tasks t ON t.id=r.task_id
         LEFT JOIN projects p ON p.id=r.project_id
         LEFT JOIN cp_wrapper_resume j ON j.agent_run_id=r.id
         WHERE r.provider='wrapper'
           AND (?2 IS NULL OR r.agent_name=?2)
           AND (?3=0 OR r.status='running')
           AND (?4 IS NULL OR p.name=?4 OR p.path=?4)
           AND (?5 IS NULL OR instr(lower(t.title),lower(?5))>0 OR instr(lower(t.description),lower(?5))>0
                OR instr(lower(COALESCE(r.summary,'')),lower(?5))>0 OR instr(r.id,?5)>0)
         ORDER BY r.started_at DESC,r.id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(
        params![
            i64::try_from(limit).unwrap_or(i64::MAX),
            agent,
            active,
            project,
            search
        ],
        |r| {
            Ok(SessionRow {
                run_id: r.get(0)?,
                agent_name: r.get(1)?,
                status: r.get(2)?,
                started_at: r.get(3)?,
                ended_at: r.get(4)?,
                exit_reason: r.get(5)?,
                summary: r.get(6)?,
                task_id: r.get(7)?,
                name: r.get(8)?,
                work: r.get(9)?,
                task_status: r.get(10)?,
                project_name: r.get(11)?,
                project_path: r.get(12)?,
                native_session_id: r.get(13)?,
                workspace_root: r.get(14)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>>>()
}

#[cfg(test)]
#[path = "wf_session_recovery_tests.rs"]
mod recovery_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn scoped_run(conn: &Connection) -> (String, i64) {
        let project_id = upsert_project(
            conn,
            "Test",
            "core",
            "/workspace/project",
            None,
            "active",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let (_, run_id) = cp_session_start(conn, "codex_kaira", Some(project_id)).unwrap();
        (run_id, project_id)
    }

    #[test]
    fn wrapper_memory_note_requires_live_matching_project_and_safe_content() {
        let conn = crate::db::tests::in_memory();
        let (run_id, project_id) = scoped_run(&conn);

        let event = cp_record_wrapper_memory_note(
            &conn,
            &run_id,
            "/workspace/project/src",
            "We decided to keep the project boundary strict.",
        )
        .unwrap();
        assert_eq!(event.agent_name, "codex_kaira");
        assert_eq!(event.project_id, project_id);

        assert!(cp_record_wrapper_memory_note(&conn, &run_id, "/other", "cross project").is_err());
        assert!(cp_record_wrapper_memory_note(
            &conn,
            &run_id,
            "/workspace/project",
            "password = 'superSecretValue123'"
        )
        .is_err());

        let (task_id, _) = cp_session_start(&conn, "codex_kaira", Some(project_id)).unwrap();
        let (ended_task, ended_run) =
            cp_session_start(&conn, "codex_kaira", Some(project_id)).unwrap();
        assert!(cp_session_end(&conn, &task_id, &ended_run, true).is_err());
        cp_session_end(&conn, &ended_task, &ended_run, true).unwrap();
        assert!(cp_record_wrapper_memory_note(
            &conn,
            &ended_run,
            "/workspace/project",
            "late note"
        )
        .is_err());
    }

    #[test]
    fn cp_session_end_with_summary_logs_a_routine_activity_event_on_success() {
        let conn = crate::db::tests::in_memory();
        let (task_id, run_id) = cp_session_start(&conn, "codex_kaira", None).unwrap();

        cp_session_end_with_summary(&conn, &task_id, &run_id, true, Some("committed 3 files"))
            .unwrap();

        let (source, tier, project, summary): (String, String, Option<String>, String) = conn
            .query_row(
                "SELECT source, tier, project, summary FROM activity_events",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(source, "agent_run");
        assert_eq!(tier, "routine");
        assert_eq!(project, None);
        assert!(
            summary.contains("committed 3 files"),
            "summary {summary:?} must contain the passed-in text"
        );
    }

    #[test]
    fn cp_session_end_with_summary_does_not_log_activity_when_summary_is_none() {
        let conn = crate::db::tests::in_memory();
        let (task_id, run_id) = cp_session_start(&conn, "codex_kaira", None).unwrap();

        cp_session_end_with_summary(&conn, &task_id, &run_id, true, None).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn cp_session_end_with_summary_does_not_log_activity_when_summary_is_empty_string() {
        let conn = crate::db::tests::in_memory();
        let (task_id, run_id) = cp_session_start(&conn, "codex_kaira", None).unwrap();

        // An empty-string summary must be treated the same as no summary —
        // guards against loosening `.filter(|s| !s.is_empty())` to a bare
        // `if let Some(text) = summary`, which would log a hollow event
        // like "agent run completed: ".
        cp_session_end_with_summary(&conn, &task_id, &run_id, true, Some("")).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn cp_session_end_with_summary_does_not_log_activity_when_run_failed() {
        let conn = crate::db::tests::in_memory();
        let (task_id, run_id) = cp_session_start(&conn, "codex_kaira", None).unwrap();

        // A failed run with a non-empty summary must not be logged as a
        // routine activity event — guards against dropping the `success`
        // check and logging on every completion regardless of outcome.
        cp_session_end_with_summary(&conn, &task_id, &run_id, false, Some("blew up")).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[cfg(test)]
#[path = "wf_session_tracking_tests.rs"]
mod tracking_tests;
