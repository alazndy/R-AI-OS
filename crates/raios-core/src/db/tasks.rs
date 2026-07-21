use rusqlite::{params, Connection, Result};
// ─── Tasks ───────────────────────────────────────────────────────────────────

// ── Legacy tasks table (compat-only; use cp_* for new work) ──────────────────

pub struct DbTask {
    pub id: i64,
    pub text: String,
    pub completed: bool,
    pub agent: Option<String>,
    pub project: Option<String>,
}

pub fn load_tasks_db(conn: &Connection) -> Result<Vec<DbTask>> {
    let mut stmt =
        conn.prepare("SELECT id, text, completed, agent, project FROM tasks ORDER BY id")?;
    let rows = stmt.query_map([], |row| {
        Ok(DbTask {
            id: row.get(0)?,
            text: row.get(1)?,
            completed: row.get::<_, i64>(2)? != 0,
            agent: row.get(3)?,
            project: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn insert_task(
    conn: &Connection,
    text: &str,
    agent: Option<&str>,
    project: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO tasks (text, agent, project) VALUES (?1, ?2, ?3)",
        params![text, agent, project],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn toggle_task(conn: &Connection, id: i64, completed: bool) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET completed = ?1 WHERE id = ?2",
        params![completed as i64, id],
    )?;
    Ok(())
}

// ── Canonical personal tasks (cp_tasks with plan_id IS NULL) ─────────────────

pub struct PersonalTaskInput {
    pub id: Option<String>,
    pub title: String,
    pub completed: bool,
    pub agent: Option<String>,
    pub project_name: Option<String>,
    pub display_order: i64,
}

#[derive(Debug)]
pub struct PersonalTaskRow {
    pub id: String,
    pub title: String,
    pub completed: bool,
    pub assignee_id: Option<String>,
    pub project_name: Option<String>,
    pub display_order: i64,
}

pub fn cp_list_personal_tasks(conn: &Connection) -> Result<Vec<PersonalTaskRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.status, t.assignee_id,
                li.project_name, COALESCE(li.display_order, 0) AS display_order
         FROM cp_tasks t
         LEFT JOIN cp_task_list_items li ON li.task_id = t.id
         WHERE t.plan_id IS NULL AND t.parent_task_id IS NULL AND t.status != 'cancelled'
           AND NOT EXISTS (SELECT 1 FROM cp_approvals ap WHERE ap.task_id = t.id)
         ORDER BY display_order ASC, t.created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let status: String = row.get(2)?;
        Ok(PersonalTaskRow {
            id: row.get(0)?,
            title: row.get(1)?,
            completed: status == "completed",
            assignee_id: row.get(3)?,
            project_name: row.get(4)?,
            display_order: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn cp_sync_personal_tasks(
    conn: &Connection,
    inputs: &[PersonalTaskInput],
    source_path: &str,
) -> Result<()> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    conn.execute_batch("BEGIN")?;

    let mut active_ids: Vec<String> = Vec::new();

    for (order, input) in inputs.iter().enumerate() {
        let display_order = order as i64;
        let status = if input.completed {
            "completed"
        } else {
            "queued"
        };
        let assignee_kind: Option<&str> = input.agent.as_ref().map(|_| "agent");

        let task_id: String = if let Some(ref id) = input.id {
            // Update existing row
            conn.execute(
                "UPDATE cp_tasks SET status=?1, assignee_kind=?2, assignee_id=?3, updated_at=?4
                 WHERE id=?5",
                params![status, assignee_kind, input.agent, now, id],
            )?;
            conn.execute(
                "UPDATE cp_task_list_items SET display_order=?1, project_name=?2 WHERE task_id=?3",
                params![display_order, input.project_name, id],
            )?;
            id.clone()
        } else {
            // Try to find existing row by title
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM cp_tasks
                     WHERE plan_id IS NULL AND parent_task_id IS NULL
                       AND title=?1 AND status != 'cancelled'",
                    params![input.title],
                    |r| r.get(0),
                )
                .ok();

            if let Some(existing_id) = existing {
                conn.execute(
                    "UPDATE cp_tasks SET status=?1, assignee_kind=?2, assignee_id=?3, updated_at=?4
                     WHERE id=?5",
                    params![status, assignee_kind, input.agent, now, existing_id],
                )?;
                conn.execute(
                    "UPDATE cp_task_list_items SET display_order=?1, project_name=?2 WHERE task_id=?3",
                    params![display_order, input.project_name, existing_id],
                )?;
                existing_id
            } else {
                let new_id = uuid::Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO cp_tasks
                     (id, plan_id, parent_task_id, title, description, priority, status,
                      assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
                     VALUES (?1, NULL, NULL, ?2, '', 50, ?3, ?4, ?5, '', ?6, ?6)",
                    params![new_id, input.title, status, assignee_kind, input.agent, now],
                )?;
                conn.execute(
                    "INSERT INTO cp_task_list_items
                     (task_id, source_kind, source_path, display_order, project_name, created_at)
                     VALUES (?1, 'markdown', ?2, ?3, ?4, ?5)",
                    params![new_id, source_path, display_order, input.project_name, now],
                )?;
                new_id
            }
        };

        active_ids.push(task_id);
    }

    // Cancel personal tasks that are no longer in the input list.
    // Scoped to rows that have a cp_task_list_items entry so swarm/file_approval
    // tasks (which have plan_id IS NULL but no list_items row) are never touched.
    if active_ids.is_empty() {
        conn.execute(
            "UPDATE cp_tasks SET status='cancelled', updated_at=?1
             WHERE plan_id IS NULL AND parent_task_id IS NULL AND status != 'cancelled'
               AND EXISTS (SELECT 1 FROM cp_task_list_items li WHERE li.task_id = cp_tasks.id)",
            params![now],
        )?;
    } else {
        let placeholders: String = active_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "UPDATE cp_tasks SET status='cancelled', updated_at=?1
             WHERE plan_id IS NULL AND parent_task_id IS NULL AND status != 'cancelled'
               AND EXISTS (SELECT 1 FROM cp_task_list_items li WHERE li.task_id = cp_tasks.id)
               AND id NOT IN ({})",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        stmt.execute(rusqlite::params_from_iter(
            std::iter::once(now.as_str()).chain(active_ids.iter().map(|s| s.as_str())),
        ))?;
    }

    conn.execute_batch("COMMIT")?;
    Ok(())
}

pub fn cp_rebuild_personal_markdown(conn: &Connection, dev_ops: &std::path::Path) -> Result<()> {
    let rows = cp_list_personal_tasks(conn)?;
    let mut out = String::from("# Dev Ops Tasks\n\n");
    for row in &rows {
        let mark = if row.completed { "x" } else { " " };
        let mut line = format!("- [{}] {}", mark, row.title);
        if let Some(ref a) = row.assignee_id {
            line.push_str(&format!(" @{}", a));
        }
        if let Some(ref p) = row.project_name {
            line.push_str(&format!(" #{}", p));
        }
        out.push_str(&line);
        out.push('\n');
    }
    let path = dev_ops.join("tasks.md");
    std::fs::write(path, out).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    Ok(())
}
