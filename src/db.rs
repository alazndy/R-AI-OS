use rusqlite::{params, Connection, OptionalExtension, Result};
use std::path::Path;

// ─── Open & migrate ──────────────────────────────────────────────────────────

pub fn open_db() -> Result<Connection> {
    let db_path = db_path();
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(&db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate_existing(&conn)?;
    Ok(conn)
}

fn db_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("raios")
        .join("workspace.db")
}

fn migrate(conn: &Connection) -> Result<()> {
    // Idempotent column additions for existing DBs (errors mean column already exists)
    let _ = conn.execute_batch(
        "ALTER TABLE health_cache ADD COLUMN refactor_grade TEXT NOT NULL DEFAULT '-'",
    );
    // Normalize any legacy garbage status values → 'active' (idempotent)
    let _ = conn.execute_batch(
        "UPDATE projects SET status = 'active' WHERE status NOT IN ('production','active','early','legacy')",
    );
    let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN refactor_score INTEGER");
    let _ = conn.execute_batch(
        "ALTER TABLE health_cache ADD COLUMN refactor_high INTEGER NOT NULL DEFAULT 0",
    );

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            category    TEXT NOT NULL DEFAULT '',
            path        TEXT UNIQUE NOT NULL,
            github      TEXT,
            status      TEXT NOT NULL DEFAULT 'active',
            stars       INTEGER,
            last_commit TEXT,
            version     TEXT,
            nickname    TEXT,
            updated_at  TEXT DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS health_cache (
            project_id       INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
            compliance_grade TEXT NOT NULL DEFAULT '-',
            compliance_score INTEGER,
            security_grade   TEXT,
            security_score   INTEGER,
            security_issues  INTEGER NOT NULL DEFAULT 0,
            security_critical INTEGER NOT NULL DEFAULT 0,
            git_dirty        INTEGER NOT NULL DEFAULT 0,
            has_memory       INTEGER NOT NULL DEFAULT 0,
            has_sigmap       INTEGER NOT NULL DEFAULT 0,
            remote_url       TEXT,
            scanned_at       TEXT DEFAULT (datetime('now')),
            refactor_grade   TEXT NOT NULL DEFAULT '-',
            refactor_score   INTEGER,
            refactor_high    INTEGER NOT NULL DEFAULT 0
        );

        -- COMPAT CACHE: tasks is superseded by cp_tasks (plan_id IS NULL). Do not read for new work.
        CREATE TABLE IF NOT EXISTS tasks (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            text       TEXT NOT NULL,
            completed  INTEGER NOT NULL DEFAULT 0,
            agent      TEXT,
            project    TEXT,
            created_at TEXT DEFAULT (datetime('now'))
        );

        -- Created unconditionally regardless of --features cortex.
        -- SQLite has no overhead for an empty table, and conditional schema
        -- migration would require versioned DDL. The cortex feature flag gates
        -- the code that writes here, not the schema itself.
        CREATE TABLE IF NOT EXISTS cortex_chunks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            path        TEXT    NOT NULL,
            mtime_secs  INTEGER NOT NULL,
            start_line  INTEGER NOT NULL,
            chunk_text  TEXT    NOT NULL,
            embedding   BLOB    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cortex_path ON cortex_chunks(path);
    ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS bm25_files (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            path       TEXT UNIQUE NOT NULL,
            mtime_secs INTEGER NOT NULL,
            doc_length INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS bm25_postings (
            token   TEXT    NOT NULL,
            file_id INTEGER NOT NULL REFERENCES bm25_files(id) ON DELETE CASCADE,
            line_no INTEGER NOT NULL,
            snippet TEXT    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_bm25_token ON bm25_postings(token);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sessions (
            id          TEXT PRIMARY KEY,
            agent       TEXT NOT NULL,
            project     TEXT,
            started_at  TEXT NOT NULL,
            ended_at    TEXT,
            summary     TEXT
        );

        CREATE TABLE IF NOT EXISTS session_events (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            event_type  TEXT NOT NULL,
            data        TEXT NOT NULL DEFAULT '',
            timestamp   TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_se_session ON session_events(session_id);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS instinct_candidates (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            project_name TEXT NOT NULL,
            command      TEXT NOT NULL,
            outcome      TEXT NOT NULL,
            suggestion   TEXT NOT NULL,
            status       TEXT NOT NULL DEFAULT 'pending',
            created_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- COMPAT CACHE: task_graphs is rebuilt from cp_task_graphs; not source of truth.
        CREATE TABLE IF NOT EXISTS task_graphs (
            id          TEXT PRIMARY KEY,
            goal        TEXT NOT NULL,
            agent       TEXT NOT NULL,
            status      TEXT NOT NULL DEFAULT 'pending',
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            completed_at TEXT
        );

        -- COMPAT CACHE: task_graph_nodes is rebuilt from cp_task_graph_nodes + cp_tasks; not source of truth.
        CREATE TABLE IF NOT EXISTS task_graph_nodes (
            id          TEXT NOT NULL,
            graph_id    TEXT NOT NULL REFERENCES task_graphs(id) ON DELETE CASCADE,
            description TEXT NOT NULL,
            shell_cmd   TEXT NOT NULL,
            deps        TEXT NOT NULL DEFAULT '[]',
            status      TEXT NOT NULL DEFAULT 'pending',
            factory_job_id TEXT,
            result      TEXT,
            error       TEXT,
            PRIMARY KEY (graph_id, id)
        );
        CREATE INDEX IF NOT EXISTS idx_tgn_graph ON task_graph_nodes(graph_id);

        CREATE TABLE IF NOT EXISTS swarm_tasks (
            id            TEXT PRIMARY KEY,
            project_name  TEXT NOT NULL,
            project_path  TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            branch_name   TEXT NOT NULL,
            description   TEXT NOT NULL,
            agent         TEXT NOT NULL,
            status        TEXT NOT NULL DEFAULT 'initializing',
            created_at    TEXT NOT NULL DEFAULT (datetime('now')),
            completed_at  TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_swarm_status ON swarm_tasks(status);

        CREATE TABLE IF NOT EXISTS cp_tasks (
            id TEXT PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
            plan_id TEXT,
            parent_task_id TEXT REFERENCES cp_tasks(id) ON DELETE SET NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 50,
            status TEXT NOT NULL,
            assignee_kind TEXT,
            assignee_id TEXT,
            acceptance_criteria TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cp_tasks_project ON cp_tasks(project_id);
        CREATE INDEX IF NOT EXISTS idx_cp_tasks_status ON cp_tasks(status);

        CREATE TABLE IF NOT EXISTS cp_agent_runs (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
            provider TEXT NOT NULL,
            agent_name TEXT NOT NULL,
            run_contract_id TEXT NOT NULL,
            attempt INTEGER NOT NULL DEFAULT 1,
            status TEXT NOT NULL,
            started_at TEXT,
            ended_at TEXT,
            exit_reason TEXT,
            summary TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cp_runs_task ON cp_agent_runs(task_id);
        CREATE INDEX IF NOT EXISTS idx_cp_runs_status ON cp_agent_runs(status);

        CREATE TABLE IF NOT EXISTS cp_artifacts (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            agent_run_id TEXT NOT NULL REFERENCES cp_agent_runs(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            status TEXT NOT NULL,
            path TEXT,
            content_ref TEXT,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cp_artifacts_task ON cp_artifacts(task_id);
        CREATE INDEX IF NOT EXISTS idx_cp_artifacts_status ON cp_artifacts(status);

        CREATE TABLE IF NOT EXISTS cp_approvals (
            id TEXT PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
            task_id TEXT REFERENCES cp_tasks(id) ON DELETE SET NULL,
            agent_run_id TEXT REFERENCES cp_agent_runs(id) ON DELETE SET NULL,
            artifact_id TEXT REFERENCES cp_artifacts(id) ON DELETE SET NULL,
            approval_type TEXT NOT NULL,
            reason TEXT NOT NULL,
            status TEXT NOT NULL,
            requested_at TEXT NOT NULL,
            resolved_at TEXT,
            resolved_by TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cp_approvals_status ON cp_approvals(status);

        CREATE TABLE IF NOT EXISTS cp_budget_ledger (
            id TEXT PRIMARY KEY,
            scope_kind TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            provider TEXT,
            metric TEXT NOT NULL,
            limit_value REAL,
            used_value REAL,
            remaining_value REAL,
            reset_at TEXT,
            confidence TEXT NOT NULL,
            source TEXT NOT NULL,
            observed_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cp_budget_scope ON cp_budget_ledger(scope_kind, scope_id);

        CREATE TABLE IF NOT EXISTS cp_task_edges (
            graph_id TEXT NOT NULL REFERENCES task_graphs(id) ON DELETE CASCADE,
            task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            depends_on_task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            edge_kind TEXT NOT NULL DEFAULT 'blocks',
            created_at TEXT NOT NULL,
            PRIMARY KEY (graph_id, task_id, depends_on_task_id)
        );
        CREATE INDEX IF NOT EXISTS idx_cp_task_edges_graph ON cp_task_edges(graph_id);
        CREATE INDEX IF NOT EXISTS idx_cp_task_edges_task ON cp_task_edges(task_id);

        CREATE TABLE IF NOT EXISTS cp_task_graph_nodes (
            graph_id TEXT NOT NULL REFERENCES task_graphs(id) ON DELETE CASCADE,
            node_id TEXT NOT NULL,
            task_id TEXT NOT NULL REFERENCES cp_tasks(id) ON DELETE CASCADE,
            agent_run_id TEXT NOT NULL REFERENCES cp_agent_runs(id) ON DELETE CASCADE,
            shell_cmd TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (graph_id, node_id),
            UNIQUE (graph_id, task_id)
        );
        CREATE INDEX IF NOT EXISTS idx_cp_task_graph_nodes_graph ON cp_task_graph_nodes(graph_id);
        CREATE INDEX IF NOT EXISTS idx_cp_task_graph_nodes_task ON cp_task_graph_nodes(task_id);

        CREATE TABLE IF NOT EXISTS cp_task_graphs (
            graph_id TEXT PRIMARY KEY REFERENCES task_graphs(id) ON DELETE CASCADE,
            goal TEXT NOT NULL,
            agent TEXT NOT NULL,
            created_at TEXT NOT NULL,
            completed_at TEXT
        );

        CREATE TABLE IF NOT EXISTS cp_task_list_items (
            task_id       TEXT PRIMARY KEY REFERENCES cp_tasks(id) ON DELETE CASCADE,
            source_kind   TEXT NOT NULL DEFAULT 'markdown',
            source_path   TEXT NOT NULL DEFAULT '',
            display_order INTEGER NOT NULL DEFAULT 0,
            project_name  TEXT,
            created_at    TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_cp_task_list_items_order ON cp_task_list_items(display_order);

        CREATE TABLE IF NOT EXISTS cp_run_contracts (
            id                   TEXT PRIMARY KEY,
            task_id              TEXT REFERENCES cp_tasks(id) ON DELETE SET NULL,
            workspace_root       TEXT NOT NULL DEFAULT '',
            allowed_paths_json   TEXT NOT NULL DEFAULT '[]',
            blocked_paths_json   TEXT NOT NULL DEFAULT '[]',
            allowed_tools_json   TEXT NOT NULL DEFAULT '[]',
            network_policy_json  TEXT NOT NULL DEFAULT '{}',
            token_budget         INTEGER,
            time_budget_secs     INTEGER,
            cpu_budget_pct       REAL,
            memory_budget_mb     INTEGER,
            expected_artifacts_json TEXT NOT NULL DEFAULT '[]',
            success_criteria_json   TEXT NOT NULL DEFAULT '{}',
            escalation_policy_json  TEXT NOT NULL DEFAULT '{}',
            created_at           TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_cp_run_contracts_task ON cp_run_contracts(task_id);

        CREATE TABLE IF NOT EXISTS cp_provider_capabilities (
            provider                      TEXT PRIMARY KEY,
            supports_tool_calling         INTEGER NOT NULL DEFAULT 0,
            supports_patch_diff           INTEGER NOT NULL DEFAULT 0,
            supports_long_running         INTEGER NOT NULL DEFAULT 0,
            supports_streaming            INTEGER NOT NULL DEFAULT 0,
            supports_exact_quota_visibility INTEGER NOT NULL DEFAULT 0,
            updated_at                    TEXT NOT NULL DEFAULT (datetime('now'))
        );
        ",
    )?;

    // ─── Audit Ledger (Faz 3: Hash-Chain Tamper Detection) ────────────────────
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS audit_log (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp  TEXT    NOT NULL DEFAULT (datetime('now','utc')),
            event_type TEXT    NOT NULL,
            actor      TEXT    NOT NULL DEFAULT 'raios',
            data       TEXT    NOT NULL DEFAULT '',
            prev_hash  TEXT    NOT NULL DEFAULT '',
            hash       TEXT    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_log(timestamp);
        ",
    )?;

    Ok(())
}

pub fn migrate_existing(conn: &Connection) -> Result<()> {
    migrate(conn)
}

// ─── Migration from entities.json ────────────────────────────────────────────

/// One-time import from entities.json → SQLite. Deletes json after success.
pub fn import_from_json(dev_ops: &Path, conn: &Connection) -> usize {
    let json_path = dev_ops.join("entities.json");
    if !json_path.exists() {
        return 0;
    }

    #[derive(serde::Deserialize)]
    struct EntitiesFile {
        #[serde(default)]
        projects: Vec<LegacyProject>,
    }
    #[derive(serde::Deserialize)]
    struct LegacyProject {
        name: String,
        #[serde(default)]
        category: String,
        local_path: std::path::PathBuf,
        github: Option<String>,
        #[serde(default = "default_status")]
        status: String,
        stars: Option<u32>,
        last_commit: Option<String>,
        version: Option<String>,
        version_nickname: Option<String>,
    }
    fn default_status() -> String {
        "active".into()
    }

    let content = match std::fs::read_to_string(&json_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let file: EntitiesFile = match serde_json::from_str(&content) {
        Ok(f) => f,
        Err(_) => return 0,
    };

    let mut imported = 0;
    for p in &file.projects {
        if !p.local_path.exists() {
            continue;
        }
        let path_str = p.local_path.to_string_lossy().to_string();
        let result = conn.execute(
            "INSERT OR IGNORE INTO projects (name, category, path, github, status, stars, last_commit, version, nickname)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                p.name, p.category, path_str, p.github,
                p.status,
                p.stars.map(|s| s as i64),
                p.last_commit, p.version, p.version_nickname,
            ],
        );
        if result.is_ok() {
            imported += 1;
        }
    }

    if imported > 0 {
        let _ = std::fs::remove_file(&json_path);
    }
    imported
}

// ─── Project CRUD ─────────────────────────────────────────────────────────────

pub struct DbProject {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub path: String,
    pub github: Option<String>,
    pub status: String,
    pub stars: Option<i64>,
    pub last_commit: Option<String>,
    pub version: Option<String>,
    pub nickname: Option<String>,
}

pub fn load_all_projects(conn: &Connection) -> Result<Vec<DbProject>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, category, path, github, status, stars, last_commit, version, nickname
         FROM projects ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(DbProject {
            id: row.get(0)?,
            name: row.get(1)?,
            category: row.get(2)?,
            path: row.get(3)?,
            github: row.get(4)?,
            status: row.get(5)?,
            stars: row.get(6)?,
            last_commit: row.get(7)?,
            version: row.get(8)?,
            nickname: row.get(9)?,
        })
    })?;
    rows.collect()
}

#[allow(clippy::too_many_arguments)]
pub fn upsert_project(
    conn: &Connection,
    name: &str,
    category: &str,
    path: &str,
    github: Option<&str>,
    status: &str,
    stars: Option<i64>,
    last_commit: Option<&str>,
    version: Option<&str>,
    nickname: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO projects (name, category, path, github, status, stars, last_commit, version, nickname)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(path) DO UPDATE SET
             name=excluded.name, category=excluded.category,
             github=COALESCE(excluded.github, github),
             status=CASE WHEN excluded.status IN ('production','active','early','legacy') THEN excluded.status ELSE status END,
             stars=COALESCE(excluded.stars, stars),
             last_commit=COALESCE(excluded.last_commit, last_commit),
             version=COALESCE(excluded.version, version),
             nickname=COALESCE(excluded.nickname, nickname),
             updated_at=datetime('now')",
        params![name, category, path, github, status, stars, last_commit, version, nickname],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn project_id_for_path(conn: &Connection, path: &str) -> Option<i64> {
    conn.query_row(
        "SELECT id FROM projects WHERE path = ?1",
        params![path],
        |row| row.get(0),
    )
    .ok()
}

// ─── Health cache ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn upsert_health(
    conn: &Connection,
    project_id: i64,
    compliance_grade: &str,
    compliance_score: Option<u8>,
    security_grade: Option<&str>,
    security_score: Option<u8>,
    security_issues: usize,
    security_critical: usize,
    git_dirty: bool,
    has_memory: bool,
    has_sigmap: bool,
    remote_url: Option<&str>,
    refactor_grade: &str,
    refactor_score: u8,
    refactor_high: usize,
) -> Result<()> {
    conn.execute(
        "INSERT INTO health_cache
            (project_id, compliance_grade, compliance_score, security_grade, security_score,
             security_issues, security_critical, git_dirty, has_memory, has_sigmap, remote_url,
             refactor_grade, refactor_score, refactor_high, scanned_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, datetime('now'))
         ON CONFLICT(project_id) DO UPDATE SET
             compliance_grade=excluded.compliance_grade,
             compliance_score=excluded.compliance_score,
             security_grade=COALESCE(excluded.security_grade, security_grade),
             security_score=COALESCE(excluded.security_score, security_score),
             security_issues=excluded.security_issues,
             security_critical=excluded.security_critical,
             git_dirty=excluded.git_dirty,
             has_memory=excluded.has_memory,
             has_sigmap=excluded.has_sigmap,
             remote_url=COALESCE(excluded.remote_url, remote_url),
             refactor_grade=excluded.refactor_grade,
             refactor_score=excluded.refactor_score,
             refactor_high=excluded.refactor_high,
             scanned_at=datetime('now')",
        params![
            project_id,
            compliance_grade,
            compliance_score.map(|s| s as i64),
            security_grade,
            security_score.map(|s| s as i64),
            security_issues as i64,
            security_critical as i64,
            git_dirty as i64,
            has_memory as i64,
            has_sigmap as i64,
            remote_url,
            refactor_grade,
            refactor_score as i64,
            refactor_high as i64,
        ],
    )?;
    Ok(())
}

// ─── Stats query ─────────────────────────────────────────────────────────────

pub struct PortfolioStats {
    pub total: i64,
    pub active: i64,
    pub archived: i64,
    pub dirty: i64,
    pub no_memory: i64,
    pub no_sigmap: i64,
    pub no_github: i64,
    pub avg_compliance: f64,
    pub avg_security: f64,
    pub grade_a: i64,
    pub grade_b: i64,
    pub grade_c: i64,
    pub grade_d: i64,
}

pub fn query_stats(conn: &Connection) -> Result<PortfolioStats> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?;
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE status = 'active'",
        [],
        |r| r.get(0),
    )?;
    let archived: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE status IN ('archived','legacy')",
        [],
        |r| r.get(0),
    )?;
    let dirty: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE git_dirty = 1",
        [],
        |r| r.get(0),
    )?;
    let no_memory: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE has_memory = 0",
        [],
        |r| r.get(0),
    )?;
    let no_sigmap: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE has_sigmap = 0",
        [],
        |r| r.get(0),
    )?;
    let no_github: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE github IS NULL",
        [],
        |r| r.get(0),
    )?;
    let avg_compliance: f64 = conn.query_row(
        "SELECT COALESCE(AVG(compliance_score), 0) FROM health_cache",
        [],
        |r| r.get(0),
    )?;
    let avg_security: f64 = conn.query_row(
        "SELECT COALESCE(AVG(security_score), 0) FROM health_cache",
        [],
        |r| r.get(0),
    )?;
    let grade_a: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE compliance_grade = 'A'",
        [],
        |r| r.get(0),
    )?;
    let grade_b: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE compliance_grade = 'B'",
        [],
        |r| r.get(0),
    )?;
    let grade_c: i64 = conn.query_row(
        "SELECT COUNT(*) FROM health_cache WHERE compliance_grade = 'C'",
        [],
        |r| r.get(0),
    )?;
    let grade_d: i64 = conn.query_row("SELECT COUNT(*) FROM health_cache WHERE compliance_grade NOT IN ('A','B','C') AND compliance_grade != '-'", [], |r| r.get(0))?;

    Ok(PortfolioStats {
        total,
        active,
        archived,
        dirty,
        no_memory,
        no_sigmap,
        no_github,
        avg_compliance,
        avg_security,
        grade_a,
        grade_b,
        grade_c,
        grade_d,
    })
}

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
        let status = if input.completed { "completed" } else { "queued" };
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
            std::iter::once(now.as_str())
                .chain(active_ids.iter().map(|s| s.as_str())),
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

// ── Budget enforcement ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetGate {
    /// Proceed normally.
    Allow,
    /// Budget data is unreliable — allow but log.
    AllowUnknown,
    /// Budget is strained; defer optional work.
    SoftDefer,
    /// Budget is exhausted; do not start this work.
    HardBlock { metric: String, scope: String },
}

impl BudgetGate {
    pub fn is_blocked(&self) -> bool {
        matches!(self, BudgetGate::HardBlock { .. })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn cp_upsert_budget_ledger(
    conn: &Connection,
    scope_kind: &str,
    scope_id: &str,
    provider: Option<&str>,
    metric: &str,
    limit_value: Option<f64>,
    used_value: Option<f64>,
    remaining_value: Option<f64>,
    confidence: &str,
    source: &str,
) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(
        "INSERT INTO cp_budget_ledger
         (id, scope_kind, scope_id, provider, metric, limit_value, used_value,
          remaining_value, confidence, source, observed_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id, scope_kind, scope_id, provider, metric,
            limit_value, used_value, remaining_value,
            confidence, source, now
        ],
    )?;
    Ok(())
}

/// Check whether a provider-level budget gate allows a task to proceed.
/// Only hard-blocks when remaining_value == 0 AND confidence is 'exact' or 'estimated'.
pub fn cp_check_provider_budget_gate(conn: &Connection, provider: &str) -> Result<BudgetGate> {
    // Check most recent ledger row for this provider with metric = 'tokens'
    let row: Option<(Option<f64>, Option<f64>, String)> = conn
        .query_row(
            "SELECT remaining_value, limit_value, confidence
             FROM cp_budget_ledger
             WHERE scope_kind = 'provider' AND scope_id = ?1 AND metric = 'tokens'
             ORDER BY observed_at DESC LIMIT 1",
            params![provider],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?)),
        )
        .optional()?;

    match row {
        None => Ok(BudgetGate::AllowUnknown),
        Some((_, _, confidence)) if confidence == "unavailable" => Ok(BudgetGate::AllowUnknown),
        Some((Some(remaining), Some(_limit), confidence))
            if remaining <= 0.0 && (confidence == "exact" || confidence == "estimated") =>
        {
            Ok(BudgetGate::HardBlock {
                metric: "tokens".into(),
                scope: format!("provider:{}", provider),
            })
        }
        Some((Some(remaining), Some(limit_val), _)) if limit_val > 0.0 && remaining / limit_val < 0.1 => {
            Ok(BudgetGate::SoftDefer)
        }
        _ => Ok(BudgetGate::Allow),
    }
}

/// Check run-contract budget gates: token_budget and time_budget from the contract.
pub fn cp_check_contract_budget_gate(conn: &Connection, task_id: &str) -> Result<BudgetGate> {
    let contract = cp_get_run_contract_for_agent_run(conn, task_id)?;
    match contract {
        None => Ok(BudgetGate::AllowUnknown),
        Some(c) => {
            // If contract specifies a token_budget of 0, block immediately
            if c.token_budget == Some(0) {
                return Ok(BudgetGate::HardBlock {
                    metric: "tokens".into(),
                    scope: format!("contract:{}", c.id),
                });
            }
            Ok(BudgetGate::Allow)
        }
    }
}

// ── Phase 6: Provider normalization ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub provider: String,
    pub supports_tool_calling: bool,
    pub supports_patch_diff: bool,
    pub supports_long_running: bool,
    pub supports_streaming: bool,
    pub supports_exact_quota_visibility: bool,
}

impl ProviderCapabilities {
    pub fn seed_known() -> Vec<Self> {
        vec![
            Self {
                provider: "claude".into(),
                supports_tool_calling: true,
                supports_patch_diff: true,
                supports_long_running: true,
                supports_streaming: true,
                supports_exact_quota_visibility: false,
            },
            Self {
                provider: "gemini".into(),
                supports_tool_calling: true,
                supports_patch_diff: false,
                supports_long_running: false,
                supports_streaming: true,
                supports_exact_quota_visibility: false,
            },
            Self {
                provider: "codex".into(),
                supports_tool_calling: false,
                supports_patch_diff: true,
                supports_long_running: false,
                supports_streaming: false,
                supports_exact_quota_visibility: true,
            },
            Self {
                provider: "swarm".into(),
                supports_tool_calling: true,
                supports_patch_diff: true,
                supports_long_running: true,
                supports_streaming: false,
                supports_exact_quota_visibility: false,
            },
            Self {
                provider: "shell".into(),
                supports_tool_calling: false,
                supports_patch_diff: false,
                supports_long_running: true,
                supports_streaming: true,
                supports_exact_quota_visibility: true,
            },
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFailureKind {
    Auth,
    Quota,
    Timeout,
    Sandbox,
    ToolError,
    HumanRejection,
    ProviderUnavailable,
    Unknown(String),
}

impl ProviderFailureKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Auth => "auth",
            Self::Quota => "quota",
            Self::Timeout => "timeout",
            Self::Sandbox => "sandbox",
            Self::ToolError => "tool_error",
            Self::HumanRejection => "human_rejection",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::Unknown(_) => "unknown",
        }
    }

    pub fn from_stored(s: &str) -> Self {
        match s {
            "auth" => Self::Auth,
            "quota" => Self::Quota,
            "timeout" => Self::Timeout,
            "sandbox" => Self::Sandbox,
            "tool_error" => Self::ToolError,
            "human_rejection" => Self::HumanRejection,
            "provider_unavailable" => Self::ProviderUnavailable,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn classify(error: &str) -> Self {
        let lower = error.to_lowercase();
        if lower.contains("auth")
            || lower.contains("401")
            || lower.contains("403")
            || lower.contains("unauthorized")
            || lower.contains("api key")
        {
            Self::Auth
        } else if lower.contains("quota")
            || lower.contains("rate limit")
            || lower.contains("429")
            || lower.contains("token limit")
            || lower.contains("context length")
        {
            Self::Quota
        } else if lower.contains("timeout") || lower.contains("timed out") || lower.contains("deadline") {
            Self::Timeout
        } else if lower.contains("sandbox")
            || lower.contains("permission denied")
            || lower.contains("access denied")
        {
            Self::Sandbox
        } else if lower.contains("tool")
            || lower.contains("function call")
            || lower.contains("invalid argument")
        {
            Self::ToolError
        } else if lower.contains("rejected") || lower.contains("declined") || lower.contains("human") {
            Self::HumanRejection
        } else if lower.contains("unavailable")
            || lower.contains("503")
            || lower.contains("connection refused")
            || lower.contains("network")
        {
            Self::ProviderUnavailable
        } else {
            Self::Unknown(error.to_string())
        }
    }
}

pub fn cp_upsert_provider_capabilities(conn: &Connection, caps: &ProviderCapabilities) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "INSERT INTO cp_provider_capabilities
            (provider, supports_tool_calling, supports_patch_diff, supports_long_running,
             supports_streaming, supports_exact_quota_visibility, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(provider) DO UPDATE SET
            supports_tool_calling          = excluded.supports_tool_calling,
            supports_patch_diff            = excluded.supports_patch_diff,
            supports_long_running          = excluded.supports_long_running,
            supports_streaming             = excluded.supports_streaming,
            supports_exact_quota_visibility = excluded.supports_exact_quota_visibility,
            updated_at                     = excluded.updated_at",
        params![
            caps.provider,
            caps.supports_tool_calling as i64,
            caps.supports_patch_diff as i64,
            caps.supports_long_running as i64,
            caps.supports_streaming as i64,
            caps.supports_exact_quota_visibility as i64,
            now,
        ],
    )?;
    Ok(())
}

pub fn cp_get_provider_capabilities(
    conn: &Connection,
    provider: &str,
) -> Result<Option<ProviderCapabilities>> {
    conn.query_row(
        "SELECT provider, supports_tool_calling, supports_patch_diff, supports_long_running,
                supports_streaming, supports_exact_quota_visibility
         FROM cp_provider_capabilities WHERE provider = ?1",
        params![provider],
        |r| {
            Ok(ProviderCapabilities {
                provider: r.get(0)?,
                supports_tool_calling: r.get::<_, i64>(1)? != 0,
                supports_patch_diff: r.get::<_, i64>(2)? != 0,
                supports_long_running: r.get::<_, i64>(3)? != 0,
                supports_streaming: r.get::<_, i64>(4)? != 0,
                supports_exact_quota_visibility: r.get::<_, i64>(5)? != 0,
            })
        },
    )
    .optional()
}

pub fn cp_list_provider_capabilities(conn: &Connection) -> Result<Vec<ProviderCapabilities>> {
    let mut stmt = conn.prepare(
        "SELECT provider, supports_tool_calling, supports_patch_diff, supports_long_running,
                supports_streaming, supports_exact_quota_visibility
         FROM cp_provider_capabilities ORDER BY provider",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProviderCapabilities {
                provider: r.get(0)?,
                supports_tool_calling: r.get::<_, i64>(1)? != 0,
                supports_patch_diff: r.get::<_, i64>(2)? != 0,
                supports_long_running: r.get::<_, i64>(3)? != 0,
                supports_streaming: r.get::<_, i64>(4)? != 0,
                supports_exact_quota_visibility: r.get::<_, i64>(5)? != 0,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Seed known provider capabilities. Skips providers already registered (no overwrites).
pub fn cp_seed_provider_capabilities(conn: &Connection) -> Result<()> {
    for caps in ProviderCapabilities::seed_known() {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cp_provider_capabilities WHERE provider = ?1",
            params![caps.provider],
            |r| r.get(0),
        )?;
        if exists == 0 {
            cp_upsert_provider_capabilities(conn, &caps)?;
        }
    }
    Ok(())
}

/// Record a normalized failure on an agent run and transition its parent task to 'failed'.
pub fn cp_record_run_failure(
    conn: &Connection,
    agent_run_id: &str,
    kind: &ProviderFailureKind,
    detail: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_agent_runs SET status='failed', ended_at=?1, exit_reason=?2 WHERE id=?3",
        params![now, kind.as_str(), agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks SET status='failed', updated_at=?1
         WHERE id = (SELECT task_id FROM cp_agent_runs WHERE id = ?2)
           AND status NOT IN ('completed','cancelled')",
        params![now, agent_run_id],
    )?;
    if !detail.is_empty() {
        let label = kind.as_str();
        conn.execute(
            "UPDATE cp_agent_runs
             SET summary = COALESCE(summary || ' | ', '') || ?1
             WHERE id = ?2",
            params![format!("[{label}] {detail}"), agent_run_id],
        )?;
    }
    Ok(())
}

/// Returns true if the provider supports all specified capabilities.
/// Permissive when provider has no registered row.
pub fn cp_check_provider_supports(
    conn: &Connection,
    provider: &str,
    needs_tool_calling: bool,
    needs_patch_diff: bool,
    needs_long_running: bool,
) -> Result<bool> {
    match cp_get_provider_capabilities(conn, provider)? {
        None => Ok(true),
        Some(c) => Ok((!needs_tool_calling || c.supports_tool_calling)
            && (!needs_patch_diff || c.supports_patch_diff)
            && (!needs_long_running || c.supports_long_running)),
    }
}

/// Return the first provider from the preference list that satisfies all capability requirements.
pub fn cp_route_to_capable_provider(
    conn: &Connection,
    needs_tool_calling: bool,
    needs_patch_diff: bool,
    needs_long_running: bool,
) -> Result<Option<String>> {
    let providers = cp_list_provider_capabilities(conn)?;
    let preferred = ["claude", "gemini", "codex", "swarm", "shell"];
    for name in &preferred {
        if let Some(p) = providers.iter().find(|p| p.provider == *name) {
            if (!needs_tool_calling || p.supports_tool_calling)
                && (!needs_patch_diff || p.supports_patch_diff)
                && (!needs_long_running || p.supports_long_running)
            {
                return Ok(Some(p.provider.clone()));
            }
        }
    }
    for p in &providers {
        if (!needs_tool_calling || p.supports_tool_calling)
            && (!needs_patch_diff || p.supports_patch_diff)
            && (!needs_long_running || p.supports_long_running)
        {
            return Ok(Some(p.provider.clone()));
        }
    }
    Ok(None)
}

// ── Run contracts ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunContract {
    pub id: String,
    pub task_id: Option<String>,
    pub workspace_root: String,
    pub allowed_paths_json: String,
    pub blocked_paths_json: String,
    pub allowed_tools_json: String,
    pub network_policy_json: String,
    pub token_budget: Option<i64>,
    pub time_budget_secs: Option<i64>,
    pub cpu_budget_pct: Option<f64>,
    pub memory_budget_mb: Option<i64>,
    pub expected_artifacts_json: String,
    pub success_criteria_json: String,
    pub escalation_policy_json: String,
    pub created_at: String,
}

pub struct RunContractBuilder {
    task_id: Option<String>,
    workspace_root: String,
    allowed_paths: Vec<String>,
    blocked_paths: Vec<String>,
    allowed_tools: Vec<String>,
    token_budget: Option<i64>,
    time_budget_secs: Option<i64>,
}

impl RunContractBuilder {
    pub fn new(workspace_root: impl Into<String>) -> Self {
        Self {
            task_id: None,
            workspace_root: workspace_root.into(),
            allowed_paths: vec![],
            blocked_paths: vec![],
            allowed_tools: vec![],
            token_budget: None,
            time_budget_secs: None,
        }
    }

    pub fn task_id(mut self, id: impl Into<String>) -> Self {
        self.task_id = Some(id.into());
        self
    }

    pub fn allowed_paths(mut self, paths: Vec<String>) -> Self {
        self.allowed_paths = paths;
        self
    }

    pub fn blocked_paths(mut self, paths: Vec<String>) -> Self {
        self.blocked_paths = paths;
        self
    }

    pub fn allowed_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = tools;
        self
    }

    pub fn token_budget(mut self, tokens: i64) -> Self {
        self.token_budget = Some(tokens);
        self
    }

    pub fn time_budget_secs(mut self, secs: i64) -> Self {
        self.time_budget_secs = Some(secs);
        self
    }

    pub fn insert(self, conn: &Connection) -> Result<String> {
        cp_insert_run_contract(
            conn,
            self.task_id.as_deref(),
            &self.workspace_root,
            &serde_json::to_string(&self.allowed_paths).unwrap_or_else(|_| "[]".into()),
            &serde_json::to_string(&self.blocked_paths).unwrap_or_else(|_| "[]".into()),
            &serde_json::to_string(&self.allowed_tools).unwrap_or_else(|_| "[]".into()),
            self.token_budget,
            self.time_budget_secs,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub fn cp_insert_run_contract(
    conn: &Connection,
    task_id: Option<&str>,
    workspace_root: &str,
    allowed_paths_json: &str,
    blocked_paths_json: &str,
    allowed_tools_json: &str,
    token_budget: Option<i64>,
    time_budget_secs: Option<i64>,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(
        "INSERT INTO cp_run_contracts
         (id, task_id, workspace_root, allowed_paths_json, blocked_paths_json,
          allowed_tools_json, token_budget, time_budget_secs, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            task_id,
            workspace_root,
            allowed_paths_json,
            blocked_paths_json,
            allowed_tools_json,
            token_budget,
            time_budget_secs,
            now
        ],
    )?;
    Ok(id)
}

pub fn cp_get_run_contract(conn: &Connection, id: &str) -> Result<Option<RunContract>> {
    conn.query_row(
        "SELECT id, task_id, workspace_root, allowed_paths_json, blocked_paths_json,
                allowed_tools_json, network_policy_json, token_budget, time_budget_secs,
                cpu_budget_pct, memory_budget_mb, expected_artifacts_json,
                success_criteria_json, escalation_policy_json, created_at
         FROM cp_run_contracts WHERE id = ?1",
        params![id],
        |r| {
            Ok(RunContract {
                id: r.get(0)?,
                task_id: r.get(1)?,
                workspace_root: r.get(2)?,
                allowed_paths_json: r.get(3)?,
                blocked_paths_json: r.get(4)?,
                allowed_tools_json: r.get(5)?,
                network_policy_json: r.get(6)?,
                token_budget: r.get(7)?,
                time_budget_secs: r.get(8)?,
                cpu_budget_pct: r.get(9)?,
                memory_budget_mb: r.get(10)?,
                expected_artifacts_json: r.get(11)?,
                success_criteria_json: r.get(12)?,
                escalation_policy_json: r.get(13)?,
                created_at: r.get(14)?,
            })
        },
    )
    .optional()
}

/// Returns the run contract for a given agent run, if the run_contract_id is a real UUID.
pub fn cp_get_run_contract_for_agent_run(
    conn: &Connection,
    agent_run_id: &str,
) -> Result<Option<RunContract>> {
    let contract_id: Option<String> = conn
        .query_row(
            "SELECT run_contract_id FROM cp_agent_runs WHERE id = ?1",
            params![agent_run_id],
            |r| r.get(0),
        )
        .optional()?;

    match contract_id {
        Some(cid) => cp_get_run_contract(conn, &cid),
        None => Ok(None),
    }
}

// ── Canonical scheduler ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SchedulerTask {
    pub id: String,
    pub title: String,
    pub priority: i64,
    /// "task_graph" | "swarm" | "file_approval" | "personal"
    pub execution_kind: String,
    /// For task_graph: the graph_id (= plan_id). For others: None.
    pub plan_id: Option<String>,
    pub assignee_id: Option<String>,
}

/// List all cp_tasks with status='ready', ordered by priority DESC then created_at ASC.
/// Excludes tasks with an unresolved pending approval gate.
/// Tasks whose provider-level token budget is hard-blocked are moved to 'blocked' status.
/// Tasks whose assigned provider lacks required capabilities are also excluded (soft-deferred).
pub fn cp_scheduler_list_ready(conn: &Connection) -> Result<Vec<SchedulerTask>> {
    let sql = format!(
        "SELECT t.id, t.title, t.priority, {origin} AS execution_kind, t.plan_id, t.assignee_id
         FROM cp_tasks t
         WHERE t.status = 'ready'
           AND NOT EXISTS (
             SELECT 1 FROM cp_approvals ap
             WHERE ap.task_id = t.id AND ap.status = 'pending'
           )
         ORDER BY t.priority DESC, t.created_at ASC",
        origin = ORIGIN_EXPR
    );
    let mut stmt = conn.prepare(&sql)?;
    let candidates: Vec<SchedulerTask> = stmt
        .query_map([], |row| {
            Ok(SchedulerTask {
                id: row.get(0)?,
                title: row.get(1)?,
                priority: row.get(2)?,
                execution_kind: row.get(3)?,
                plan_id: row.get(4)?,
                assignee_id: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut runnable = Vec::new();
    for task in candidates {
        // Check provider-level budget gate
        let gate = task
            .assignee_id
            .as_deref()
            .map(|provider| cp_check_provider_budget_gate(conn, provider))
            .transpose()?
            .unwrap_or(BudgetGate::Allow);

        if gate.is_blocked() {
            let _ = conn.execute(
                "UPDATE cp_tasks SET status='blocked', updated_at=?1 WHERE id=?2",
                params![now, task.id],
            );
            continue;
        }

        // Check provider capability gate: agent tasks need tool_calling support.
        let needs_tools = matches!(task.execution_kind.as_str(), "swarm" | "task_graph");
        if needs_tools {
            if let Some(provider) = task.assignee_id.as_deref() {
                let capable =
                    cp_check_provider_supports(conn, provider, true, false, false)
                        .unwrap_or(true);
                if !capable {
                    // Soft-defer: leave task in 'ready' but skip this scheduling cycle
                    continue;
                }
            }
        }

        runnable.push(task);
    }
    Ok(runnable)
}

/// Returns `(graph_id, node_id)` for a task that belongs to a task graph node.
pub fn cp_task_graph_node_ids(conn: &Connection, task_id: &str) -> Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT graph_id, node_id FROM cp_task_graph_nodes WHERE task_id = ?1",
        params![task_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )
    .optional()
}

/// Returns the shell_cmd stored in cp_task_graph_nodes for a task.
pub fn cp_task_graph_shell_cmd(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT shell_cmd FROM cp_task_graph_nodes WHERE task_id = ?1",
        params![task_id],
        |r| r.get(0),
    )
    .optional()
}

// ── Unified read models ───────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct UnifiedTaskRow {
    pub id: String,
    pub title: String,
    pub status: String,
    /// "personal" | "task_graph" | "file_approval" | "swarm"
    pub origin: String,
    pub assignee_id: Option<String>,
    pub project_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ApprovalInboxRow {
    pub id: String,
    pub approval_type: String,
    pub reason: String,
    pub status: String,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub requested_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunOverviewRow {
    pub id: String,
    pub task_id: String,
    pub task_title: Option<String>,
    pub provider: String,
    pub agent_name: String,
    pub status: String,
    pub started_at: Option<String>,
}

const ORIGIN_EXPR: &str = "
    CASE
      WHEN t.plan_id IS NOT NULL THEN 'task_graph'
      WHEN EXISTS (
        SELECT 1 FROM cp_approvals ap
        WHERE ap.task_id = t.id AND ap.approval_type = 'file_write'
      ) THEN 'file_approval'
      WHEN EXISTS (
        SELECT 1 FROM cp_agent_runs ar
        WHERE ar.task_id = t.id AND ar.provider = 'swarm'
      ) THEN 'swarm'
      ELSE 'personal'
    END";

/// All non-terminal tasks from cp_tasks, ordered by updated_at DESC.
pub fn cp_query_active_tasks(conn: &Connection) -> Result<Vec<UnifiedTaskRow>> {
    let sql = format!(
        "SELECT t.id, t.title, t.status, {origin} AS origin,
                t.assignee_id, li.project_name, t.created_at, t.updated_at
         FROM cp_tasks t
         LEFT JOIN cp_task_list_items li ON li.task_id = t.id
         WHERE t.status NOT IN ('cancelled', 'completed', 'failed')
         ORDER BY t.updated_at DESC",
        origin = ORIGIN_EXPR
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(UnifiedTaskRow {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            origin: row.get(3)?,
            assignee_id: row.get(4)?,
            project_name: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    rows.collect()
}

/// All blocked tasks (status = 'blocked').
pub fn cp_query_blocked_tasks(conn: &Connection) -> Result<Vec<UnifiedTaskRow>> {
    let sql = format!(
        "SELECT t.id, t.title, t.status, {origin} AS origin,
                t.assignee_id, li.project_name, t.created_at, t.updated_at
         FROM cp_tasks t
         LEFT JOIN cp_task_list_items li ON li.task_id = t.id
         WHERE t.status = 'blocked'
         ORDER BY t.updated_at DESC",
        origin = ORIGIN_EXPR
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(UnifiedTaskRow {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            origin: row.get(3)?,
            assignee_id: row.get(4)?,
            project_name: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    rows.collect()
}

/// All pending approvals, joined with task titles.
pub fn cp_query_pending_approvals(conn: &Connection) -> Result<Vec<ApprovalInboxRow>> {
    let mut stmt = conn.prepare(
        "SELECT ap.id, ap.approval_type, ap.reason, ap.status,
                ap.task_id, t.title, ap.requested_at
         FROM cp_approvals ap
         LEFT JOIN cp_tasks t ON t.id = ap.task_id
         WHERE ap.status = 'pending'
         ORDER BY ap.requested_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ApprovalInboxRow {
            id: row.get(0)?,
            approval_type: row.get(1)?,
            reason: row.get(2)?,
            status: row.get(3)?,
            task_id: row.get(4)?,
            task_title: row.get(5)?,
            requested_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

/// All in-progress agent runs.
pub fn cp_query_active_runs(conn: &Connection) -> Result<Vec<RunOverviewRow>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.task_id, t.title, r.provider, r.agent_name, r.status, r.started_at
         FROM cp_agent_runs r
         LEFT JOIN cp_tasks t ON t.id = r.task_id
         WHERE r.status IN ('pending', 'starting', 'running', 'awaiting_input', 'awaiting_approval')
         ORDER BY r.started_at DESC NULLS LAST",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(RunOverviewRow {
            id: row.get(0)?,
            task_id: row.get(1)?,
            task_title: row.get(2)?,
            provider: row.get(3)?,
            agent_name: row.get(4)?,
            status: row.get(5)?,
            started_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

// ── Daemon snapshot ───────────────────────────────────────────────────────────

/// Point-in-time operational snapshot of the canonical control plane.
#[derive(Debug, Default)]
pub struct DaemonSnapshot {
    /// All non-terminal tasks (queued / ready / running / blocked).
    pub active_tasks: Vec<UnifiedTaskRow>,
    /// All in-progress or pending agent runs.
    pub active_runs: Vec<RunOverviewRow>,
    /// All approvals waiting for a human decision.
    pub pending_approvals: Vec<ApprovalInboxRow>,
    /// Tasks explicitly in 'blocked' status.
    pub blocked_tasks: Vec<UnifiedTaskRow>,
    /// Provider names whose budget is strained (SoftDefer) or exhausted (HardBlock).
    pub budget_deferrals: Vec<String>,
}

pub fn cp_daemon_snapshot(conn: &Connection) -> Result<DaemonSnapshot> {
    let active_tasks = cp_query_active_tasks(conn)?;
    let active_runs = cp_query_active_runs(conn)?;
    let pending_approvals = cp_query_pending_approvals(conn)?;
    let blocked_tasks = cp_query_blocked_tasks(conn)?;

    // Collect providers with a non-Allow budget gate
    let mut budget_deferrals = Vec::new();
    let mut prov_stmt = conn.prepare(
        "SELECT DISTINCT provider FROM cp_budget_ledger WHERE provider IS NOT NULL",
    )?;
    let providers: Vec<String> = prov_stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for provider in providers {
        match cp_check_provider_budget_gate(conn, &provider)? {
            BudgetGate::Allow | BudgetGate::AllowUnknown => {}
            _ => budget_deferrals.push(provider),
        }
    }

    Ok(DaemonSnapshot {
        active_tasks,
        active_runs,
        pending_approvals,
        blocked_tasks,
        budget_deferrals,
    })
}

// ── Legacy cache repair tools ─────────────────────────────────────────────────

/// Result of a divergence check between canonical state and a legacy cache.
#[derive(Debug, Default)]
pub struct DriftReport {
    /// Task ids present in canonical cp_tasks but missing from legacy cache.
    pub missing_from_cache: Vec<String>,
    /// Task ids present in legacy cache but missing from canonical cp_tasks.
    pub stale_in_cache: Vec<String>,
}

/// Check for drift between cp_tasks (canonical) and task_graph_nodes (cache) for a given graph.
pub fn cp_detect_graph_cache_drift(conn: &Connection, graph_id: &str) -> Result<DriftReport> {
    // canonical task ids for this graph
    let mut canonical_stmt = conn.prepare(
        "SELECT task_id FROM cp_task_graph_nodes WHERE graph_id = ?1",
    )?;
    let canonical_ids: std::collections::HashSet<String> = canonical_stmt
        .query_map(params![graph_id], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    // legacy cache node ids for this graph (via cp_task_id column if present)
    let mut cache_stmt = conn.prepare(
        "SELECT cp_task_id FROM task_graph_nodes WHERE graph_id = ?1 AND cp_task_id IS NOT NULL",
    )?;
    let cache_ids: std::collections::HashSet<String> = cache_stmt
        .query_map(params![graph_id], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let missing_from_cache: Vec<String> = canonical_ids
        .difference(&cache_ids)
        .cloned()
        .collect();
    let stale_in_cache: Vec<String> = cache_ids
        .difference(&canonical_ids)
        .cloned()
        .collect();

    Ok(DriftReport { missing_from_cache, stale_in_cache })
}

/// Rebuild the legacy task_graph_nodes cache for all graphs from canonical cp_* state.
/// Safe to call multiple times (idempotent).
pub fn cp_rebuild_task_graph_cache(conn: &Connection) -> Result<usize> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut rebuilt = 0usize;

    // For each cp_task_graph_nodes row, sync status from cp_tasks into task_graph_nodes
    let mut stmt = conn.prepare(
        "SELECT cgn.graph_id, cgn.node_id, cgn.task_id, t.status, t.description
         FROM cp_task_graph_nodes cgn
         JOIN cp_tasks t ON t.id = cgn.task_id",
    )?;

    let rows: Vec<(String, String, String, String, String)> = stmt
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (graph_id, node_id, _task_id, cp_status, _desc) in &rows {
        let legacy_status = match cp_status.as_str() {
            "completed" => "completed",
            "failed" | "cancelled" => "failed",
            "running" => "running",
            _ => "pending",
        };
        let updated = conn.execute(
            "UPDATE task_graph_nodes SET status = ?1
             WHERE graph_id = ?2 AND id = ?3 AND status != ?1",
            params![legacy_status, graph_id, node_id],
        )?;
        if updated > 0 {
            rebuilt += 1;
        }
    }

    // Also sync task_graphs status from cp_task_graphs
    conn.execute(
        "UPDATE task_graphs SET status = cg.status, completed_at = ?1
         FROM cp_task_graphs cg
         WHERE task_graphs.id = cg.graph_id
           AND cg.completed_at IS NOT NULL
           AND task_graphs.completed_at IS NULL",
        params![now],
    )
    .ok(); // best-effort; fails on older SQLite that doesn't support UPDATE FROM

    Ok(rebuilt)
}

pub fn project_id_for_file_path(conn: &Connection, path: &str) -> Option<i64> {
    let target = path.replace('\\', "/");
    let projects = load_all_projects(conn).ok()?;
    projects
        .into_iter()
        .filter_map(|project| {
            let root = project.path.replace('\\', "/");
            target
                .starts_with(&root)
                .then_some((project.id, root.len()))
        })
        .max_by_key(|(_, len)| *len)
        .map(|(id, _)| id)
}

pub fn create_file_change_workflow(
    conn: &Connection,
    path: &str,
    original_content: &str,
    new_content: &str,
    agent_name: &str,
) -> Result<crate::control_plane::FileChangeWorkflowIds> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let project_id = project_id_for_file_path(conn, path);
    let task_id = uuid::Uuid::new_v4().to_string();
    let agent_run_id = uuid::Uuid::new_v4().to_string();
    let artifact_id = uuid::Uuid::new_v4().to_string();
    let approval_id = uuid::Uuid::new_v4().to_string();
    let title = format!(
        "Review file change: {}",
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
    );
    let description = format!("Review and apply pending file mutation for {}", path);
    let metadata_json = serde_json::json!({
        "path": path,
        "agent_name": agent_name,
        "original_content": original_content,
        "new_content": new_content,
        "flow": "file_change_approval"
    })
    .to_string();

    conn.execute(
        "INSERT INTO cp_tasks
            (id, project_id, plan_id, parent_task_id, title, description, priority, status,
             assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, ?2, NULL, NULL, ?3, ?4, 50, 'awaiting_approval',
             'agent', ?5, 'apply approved file change safely', ?6, ?6)",
        params![task_id, project_id, title, description, agent_name, now],
    )?;

    // Create a persisted run contract scoped to this specific file path (after cp_tasks insert)
    let workspace_root = Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let allowed_paths = serde_json::to_string(&[path]).unwrap_or_else(|_| "[]".into());
    let run_contract_id = cp_insert_run_contract(
        conn,
        Some(&task_id),
        &workspace_root,
        &allowed_paths,
        "[]",
        "[]",
        None,
        None,
    )?;

    conn.execute(
        "INSERT INTO cp_agent_runs
            (id, task_id, project_id, provider, agent_name, run_contract_id, attempt, status, started_at)
         VALUES (?1, ?2, ?3, 'daemon', ?4, ?5, 1, 'awaiting_approval', ?6)",
        params![agent_run_id, task_id, project_id, agent_name, run_contract_id, now],
    )?;
    conn.execute(
        "INSERT INTO cp_artifacts
            (id, task_id, agent_run_id, kind, status, path, content_ref, metadata_json, created_at)
         VALUES (?1, ?2, ?3, 'file_change', 'submitted', ?4, NULL, ?5, ?6)",
        params![artifact_id, task_id, agent_run_id, path, metadata_json, now],
    )?;
    conn.execute(
        "INSERT INTO cp_approvals
            (id, project_id, task_id, agent_run_id, artifact_id, approval_type, reason, status, requested_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'file_write', ?6, 'pending', ?7)",
        params![
            approval_id,
            project_id,
            task_id,
            agent_run_id,
            artifact_id,
            format!("File change requested for {}", path),
            now
        ],
    )?;

    Ok(crate::control_plane::FileChangeWorkflowIds {
        task_id,
        agent_run_id,
        artifact_id,
        approval_id,
        project_id,
    })
}

pub fn mark_file_change_workflow_applied(
    conn: &Connection,
    ids: &crate::control_plane::FileChangeWorkflowIds,
    resolved_by: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals
         SET status = 'approved', resolved_at = ?1, resolved_by = ?2
         WHERE id = ?3",
        params![now, resolved_by, ids.approval_id],
    )?;
    conn.execute(
        "UPDATE cp_artifacts SET status = 'applied' WHERE id = ?1",
        params![ids.artifact_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'succeeded', ended_at = ?1, exit_reason = 'approved_and_applied'
         WHERE id = ?2",
        params![now, ids.agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'completed', updated_at = ?1
         WHERE id = ?2",
        params![now, ids.task_id],
    )?;
    Ok(())
}

pub fn mark_file_change_workflow_rejected(
    conn: &Connection,
    ids: &crate::control_plane::FileChangeWorkflowIds,
    resolved_by: &str,
    reason: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals
         SET status = 'rejected', resolved_at = ?1, resolved_by = ?2
         WHERE id = ?3",
        params![now, resolved_by, ids.approval_id],
    )?;
    conn.execute(
        "UPDATE cp_artifacts SET status = 'rejected' WHERE id = ?1",
        params![ids.artifact_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'failed', ended_at = ?1, exit_reason = ?2
         WHERE id = ?3",
        params![now, reason, ids.agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'failed', updated_at = ?1
         WHERE id = ?2",
        params![now, ids.task_id],
    )?;
    Ok(())
}

pub fn mark_file_change_workflow_apply_failed(
    conn: &Connection,
    ids: &crate::control_plane::FileChangeWorkflowIds,
    resolved_by: &str,
    reason: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals
         SET status = 'approved', resolved_at = ?1, resolved_by = ?2
         WHERE id = ?3",
        params![now, resolved_by, ids.approval_id],
    )?;
    conn.execute(
        "UPDATE cp_artifacts SET status = 'rejected' WHERE id = ?1",
        params![ids.artifact_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'failed', ended_at = ?1, exit_reason = ?2
         WHERE id = ?3",
        params![now, reason, ids.agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'failed', updated_at = ?1
         WHERE id = ?2",
        params![now, ids.task_id],
    )?;
    Ok(())
}

pub fn create_swarm_workflow(
    conn: &Connection,
    project_path: &str,
    description: &str,
    agent_name: &str,
) -> Result<crate::control_plane::FileChangeWorkflowIds> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let project_id = project_id_for_file_path(conn, project_path);
    let task_id = uuid::Uuid::new_v4().to_string();
    let agent_run_id = uuid::Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO cp_tasks
            (id, project_id, plan_id, parent_task_id, title, description, priority, status,
             assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, ?2, NULL, NULL, ?3, ?4, 60, 'queued',
             'agent', ?5, 'merge or reject swarm worktree cleanly', ?6, ?6)",
        params![
            task_id,
            project_id,
            format!("Swarm task: {}", description),
            description,
            agent_name,
            now
        ],
    )?;

    // Create a persisted run contract scoped to the project path (after cp_tasks insert)
    let allowed_paths = serde_json::to_string(&[project_path]).unwrap_or_else(|_| "[]".into());
    let run_contract_id = cp_insert_run_contract(
        conn,
        Some(&task_id),
        project_path,
        &allowed_paths,
        "[]",
        "[]",
        None,
        Some(3600),
    )?;

    conn.execute(
        "INSERT INTO cp_agent_runs
            (id, task_id, project_id, provider, agent_name, run_contract_id, attempt, status, started_at)
         VALUES (?1, ?2, ?3, 'swarm', ?4, ?5, 1, 'pending', ?6)",
        params![agent_run_id, task_id, project_id, agent_name, run_contract_id, now],
    )?;

    Ok(crate::control_plane::FileChangeWorkflowIds {
        task_id,
        agent_run_id,
        artifact_id: String::new(),
        approval_id: String::new(),
        project_id,
    })
}

pub fn mark_swarm_workflow_running(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks SET status = 'running', updated_at = ?1 WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs SET status = 'running', started_at = COALESCE(started_at, ?1) WHERE id = ?2",
        params![now, agent_run_id],
    )?;
    Ok(())
}

pub fn create_task_graph_node_workflow(
    conn: &Connection,
    graph_id: &str,
    node_id: &str,
    description: &str,
    shell_cmd: &str,
    agent_name: &str,
    ready: bool,
) -> Result<crate::control_plane::FileChangeWorkflowIds> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let task_id = uuid::Uuid::new_v4().to_string();
    let agent_run_id = uuid::Uuid::new_v4().to_string();
    let title = format!("Graph node {}: {}", node_id, description);
    let task_status = if ready { "ready" } else { "queued" };

    conn.execute(
        "INSERT INTO cp_tasks
            (id, project_id, plan_id, parent_task_id, title, description, priority, status,
             assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, NULL, ?2, NULL, ?3, ?4, 40, ?5,
             'agent', ?6, ?7, ?8, ?8)",
        params![
            task_id,
            graph_id,
            title,
            description,
            task_status,
            agent_name,
            format!("execute shell command: {}", shell_cmd),
            now
        ],
    )?;

    // Create a persisted run contract for this shell-execution task (after cp_tasks insert)
    let allowed_tools = serde_json::to_string(&["shell"]).unwrap_or_else(|_| "[]".into());
    let run_contract_id = cp_insert_run_contract(
        conn,
        Some(&task_id),
        "",
        "[]",
        "[]",
        &allowed_tools,
        None,
        Some(600),
    )?;

    conn.execute(
        "INSERT INTO cp_agent_runs
            (id, task_id, project_id, provider, agent_name, run_contract_id, attempt, status, summary)
         VALUES (?1, ?2, NULL, 'task_graph', ?3, ?4, 1, 'pending', ?5)",
        params![agent_run_id, task_id, agent_name, run_contract_id, shell_cmd],
    )?;
    conn.execute(
        "INSERT INTO cp_task_graph_nodes
            (graph_id, node_id, task_id, agent_run_id, shell_cmd, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![graph_id, node_id, task_id, agent_run_id, shell_cmd, now],
    )?;

    Ok(crate::control_plane::FileChangeWorkflowIds {
        task_id,
        agent_run_id,
        artifact_id: String::new(),
        approval_id: String::new(),
        project_id: None,
    })
}

pub fn create_task_graph_edges(
    conn: &Connection,
    graph_id: &str,
    node_task_ids: &std::collections::HashMap<String, String>,
    nodes: &[crate::task_graph::NodeSpec],
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    for node in nodes {
        let Some(task_id) = node_task_ids.get(&node.id) else {
            continue;
        };
        for dep_node_id in &node.deps {
            let Some(depends_on_task_id) = node_task_ids.get(dep_node_id) else {
                continue;
            };
            conn.execute(
                "INSERT OR IGNORE INTO cp_task_edges
                    (graph_id, task_id, depends_on_task_id, edge_kind, created_at)
                 VALUES (?1, ?2, ?3, 'blocks', ?4)",
                params![graph_id, task_id, depends_on_task_id, now],
            )?;
        }
    }

    Ok(())
}

pub fn load_control_task_statuses(
    conn: &Connection,
    task_ids: &[String],
) -> std::collections::HashMap<String, String> {
    let mut statuses = std::collections::HashMap::new();
    let mut stmt = match conn.prepare("SELECT status FROM cp_tasks WHERE id = ?1") {
        Ok(stmt) => stmt,
        Err(_) => return statuses,
    };

    for task_id in task_ids {
        if let Ok(status) = stmt.query_row(params![task_id], |row| row.get::<_, String>(0)) {
            statuses.insert(task_id.clone(), status);
        }
    }

    statuses
}

pub fn load_graph_control_task_statuses(
    conn: &Connection,
    graph_id: &str,
) -> std::collections::HashMap<String, String> {
    let task_ids = {
        let mut stmt = match conn.prepare(
            "SELECT task_id FROM cp_task_graph_nodes WHERE graph_id = ?1",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return std::collections::HashMap::new(),
        };

        stmt.query_map(params![graph_id], |row| row.get::<_, String>(0))
            .ok()
            .map(|rows| rows.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
    };

    load_control_task_statuses(conn, &task_ids)
}

pub fn load_graph_node_dependencies(
    conn: &Connection,
    graph_id: &str,
) -> std::collections::HashMap<String, Vec<String>> {
    let mut deps_by_node = std::collections::HashMap::new();
    let mut stmt = match conn.prepare(
        "SELECT node.node_id, dep_node.node_id
         FROM cp_task_edges edges
         JOIN cp_task_graph_nodes node
           ON node.graph_id = edges.graph_id AND node.task_id = edges.task_id
         JOIN cp_task_graph_nodes dep_node
           ON dep_node.graph_id = edges.graph_id AND dep_node.task_id = edges.depends_on_task_id
         WHERE edges.graph_id = ?1
         ORDER BY node.node_id, dep_node.node_id",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return deps_by_node,
    };

    let rows = match stmt.query_map(params![graph_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) {
        Ok(rows) => rows,
        Err(_) => return deps_by_node,
    };

    for (node_id, dep_id) in rows.flatten() {
        deps_by_node.entry(node_id).or_insert_with(Vec::new).push(dep_id);
    }

    deps_by_node
}

pub struct ControlGraphNodeRow {
    pub node_id: String,
    pub task_id: String,
    pub agent_run_id: String,
    pub description: String,
    pub shell_cmd: String,
    pub task_status: String,
    pub run_status: String,
    pub run_contract_id: String,
    pub summary: Option<String>,
    pub exit_reason: Option<String>,
}

pub fn load_control_graph_nodes(conn: &Connection, graph_id: &str) -> Vec<ControlGraphNodeRow> {
    let mut stmt = match conn.prepare(
        "SELECT meta.node_id, meta.task_id, meta.agent_run_id, task.description, meta.shell_cmd,
                task.status, run.status, run.run_contract_id, run.summary, run.exit_reason
         FROM cp_task_graph_nodes meta
         JOIN cp_tasks task ON task.id = meta.task_id
         JOIN cp_agent_runs run ON run.id = meta.agent_run_id
         WHERE meta.graph_id = ?1
         ORDER BY meta.node_id",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return vec![],
    };

    stmt.query_map(params![graph_id], |row| {
        Ok(ControlGraphNodeRow {
            node_id: row.get(0)?,
            task_id: row.get(1)?,
            agent_run_id: row.get(2)?,
            description: row.get(3)?,
            shell_cmd: row.get(4)?,
            task_status: row.get(5)?,
            run_status: row.get(6)?,
            run_contract_id: row.get(7)?,
            summary: row.get(8)?,
            exit_reason: row.get(9)?,
        })
    })
    .ok()
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
}

pub fn mark_control_task_ready(conn: &Connection, task_id: &str) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'ready', updated_at = ?1
         WHERE id = ?2 AND status = 'queued'",
        params![now, task_id],
    )?;
    Ok(())
}

pub fn mark_control_task_running(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    job_id: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'running', updated_at = ?1
         WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'running', started_at = COALESCE(started_at, ?1), run_contract_id = ?2
         WHERE id = ?3",
        params![now, job_id, agent_run_id],
    )?;
    Ok(())
}

pub fn mark_control_task_completed(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    result: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'completed', updated_at = ?1
         WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'succeeded', ended_at = ?1, exit_reason = 'completed', summary = ?2
         WHERE id = ?3",
        params![now, result, agent_run_id],
    )?;
    Ok(())
}

pub fn mark_control_task_failed(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    error: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'failed', updated_at = ?1
         WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'failed', ended_at = ?1, exit_reason = ?2
         WHERE id = ?3",
        params![now, error, agent_run_id],
    )?;
    Ok(())
}

pub fn ensure_swarm_review_artifacts(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    project_id: Option<i64>,
    project_path: &str,
    branch_name: &str,
    worktree_path: &str,
) -> Result<(String, String)> {
    let existing = conn.query_row(
        "SELECT id FROM cp_artifacts WHERE task_id = ?1 AND kind = 'diff' ORDER BY created_at DESC LIMIT 1",
        params![task_id],
        |row| row.get::<_, String>(0),
    ).ok();
    let existing_approval = conn.query_row(
        "SELECT id FROM cp_approvals WHERE task_id = ?1 AND approval_type = 'merge' ORDER BY requested_at DESC LIMIT 1",
        params![task_id],
        |row| row.get::<_, String>(0),
    ).ok();
    if let (Some(artifact_id), Some(approval_id)) = (existing, existing_approval) {
        return Ok((artifact_id, approval_id));
    }

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let artifact_id = uuid::Uuid::new_v4().to_string();
    let approval_id = uuid::Uuid::new_v4().to_string();
    let metadata_json = serde_json::json!({
        "project_path": project_path,
        "branch_name": branch_name,
        "worktree_path": worktree_path,
        "flow": "swarm_review"
    })
    .to_string();

    conn.execute(
        "INSERT INTO cp_artifacts
            (id, task_id, agent_run_id, kind, status, path, content_ref, metadata_json, created_at)
         VALUES (?1, ?2, ?3, 'diff', 'submitted', ?4, ?5, ?6, ?7)",
        params![
            artifact_id,
            task_id,
            agent_run_id,
            worktree_path,
            branch_name,
            metadata_json,
            now
        ],
    )?;
    conn.execute(
        "INSERT INTO cp_approvals
            (id, project_id, task_id, agent_run_id, artifact_id, approval_type, reason, status, requested_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'merge', ?6, 'pending', ?7)",
        params![
            approval_id,
            project_id,
            task_id,
            agent_run_id,
            artifact_id,
            format!("Review swarm merge for branch {}", branch_name),
            now
        ],
    )?;
    conn.execute(
        "UPDATE cp_tasks SET status = 'awaiting_approval', updated_at = ?1 WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs SET status = 'awaiting_approval' WHERE id = ?1",
        params![agent_run_id],
    )?;

    Ok((artifact_id, approval_id))
}

pub fn mark_swarm_workflow_merged(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    artifact_id: Option<&str>,
    approval_id: Option<&str>,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if let Some(artifact_id) = artifact_id.filter(|id| !id.is_empty()) {
        conn.execute(
            "UPDATE cp_artifacts SET status = 'applied' WHERE id = ?1",
            params![artifact_id],
        )?;
    }
    if let Some(approval_id) = approval_id.filter(|id| !id.is_empty()) {
        conn.execute(
            "UPDATE cp_approvals
             SET status = 'approved', resolved_at = ?1, resolved_by = 'human'
             WHERE id = ?2",
            params![now, approval_id],
        )?;
    }
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'succeeded', ended_at = ?1, exit_reason = 'merged'
         WHERE id = ?2",
        params![now, agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks SET status = 'completed', updated_at = ?1 WHERE id = ?2",
        params![now, task_id],
    )?;
    Ok(())
}

pub fn mark_swarm_workflow_rejected(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    artifact_id: Option<&str>,
    approval_id: Option<&str>,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if let Some(artifact_id) = artifact_id.filter(|id| !id.is_empty()) {
        conn.execute(
            "UPDATE cp_artifacts SET status = 'rejected' WHERE id = ?1",
            params![artifact_id],
        )?;
    }
    if let Some(approval_id) = approval_id.filter(|id| !id.is_empty()) {
        conn.execute(
            "UPDATE cp_approvals
             SET status = 'rejected', resolved_at = ?1, resolved_by = 'human'
             WHERE id = ?2",
            params![now, approval_id],
        )?;
    }
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'cancelled', ended_at = ?1, exit_reason = 'rejected'
         WHERE id = ?2",
        params![now, agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks SET status = 'cancelled', updated_at = ?1 WHERE id = ?2",
        params![now, task_id],
    )?;
    Ok(())
}

// ── Hydrated pending file-change approvals ────────────────────────────────────

/// All data needed to reconstruct a FileChangeApproval from canonical DB state.
pub struct FileChangeApprovalData {
    /// cp_approvals.id — the canonical, stable identifier.
    pub approval_id: String,
    pub task_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub artifact_id: Option<String>,
    pub path: String,
    pub original_content: String,
    pub new_content: String,
    pub agent_name: String,
}

/// Load all pending file-write approvals from canonical tables, hydrated with
/// path/content from cp_artifacts.metadata_json.
pub fn cp_load_pending_file_change_approvals(
    conn: &Connection,
) -> Result<Vec<FileChangeApprovalData>> {
    let mut stmt = conn.prepare(
        "SELECT ap.id,
                ap.task_id,
                (SELECT ar.id        FROM cp_agent_runs ar WHERE ar.task_id = ap.task_id LIMIT 1),
                (SELECT a.id         FROM cp_artifacts  a  WHERE a.task_id  = ap.task_id LIMIT 1),
                (SELECT a.metadata_json FROM cp_artifacts a WHERE a.task_id = ap.task_id LIMIT 1),
                (SELECT ar.agent_name   FROM cp_agent_runs ar WHERE ar.task_id = ap.task_id LIMIT 1)
         FROM cp_approvals ap
         WHERE ap.status = 'pending' AND ap.approval_type = 'file_write'
         ORDER BY ap.requested_at DESC",
    )?;

    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(
            |(approval_id, task_id, agent_run_id, artifact_id, meta_json, run_agent_name)| {
                let meta: serde_json::Value = meta_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default();
                FileChangeApprovalData {
                    approval_id,
                    task_id,
                    agent_run_id,
                    artifact_id,
                    path: meta["path"].as_str().unwrap_or("").to_string(),
                    original_content: meta["original_content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    new_content: meta["new_content"].as_str().unwrap_or("").to_string(),
                    agent_name: meta["agent_name"]
                        .as_str()
                        .or(run_agent_name.as_deref())
                        .unwrap_or("unknown")
                        .to_string(),
                }
            },
        )
        .collect();

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn sqlite_open_and_migrate() {
        let conn = in_memory();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn project_crud_round_trip() {
        let conn = in_memory();
        upsert_project(
            &conn,
            "TestProj",
            "devtools",
            "/tmp/test",
            None,
            "active",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let projects = load_all_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "TestProj");
    }

    #[test]
    fn upsert_is_idempotent() {
        let conn = in_memory();
        upsert_project(
            &conn,
            "P",
            "cat",
            "/tmp/p",
            Some("gh/p"),
            "active",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        upsert_project(
            &conn,
            "P-renamed",
            "cat",
            "/tmp/p",
            None,
            "active",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let projects = load_all_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "P-renamed");
        assert_eq!(projects[0].github.as_deref(), Some("gh/p")); // preserved
    }

    #[test]
    fn health_cache_upsert() {
        let conn = in_memory();
        upsert_project(
            &conn, "P", "c", "/tmp/p", None, "active", None, None, None, None,
        )
        .unwrap();
        let id = project_id_for_path(&conn, "/tmp/p").unwrap();
        upsert_health(
            &conn,
            id,
            "A",
            Some(90),
            Some("A"),
            Some(95),
            0,
            0,
            false,
            true,
            true,
            None,
            "A",
            95,
            0,
        )
        .unwrap();
        let stats = query_stats(&conn).unwrap();
        assert_eq!(stats.grade_a, 1);
    }

    #[test]
    fn task_insert_and_toggle() {
        let conn = in_memory();
        let id = insert_task(&conn, "Fix bug", Some("claude"), Some("RAIOS")).unwrap();
        toggle_task(&conn, id, true).unwrap();
        let tasks = load_tasks_db(&conn).unwrap();
        assert!(tasks[0].completed);
    }

    #[test]
    fn cortex_table_exists_after_migrate() {
        let conn = in_memory();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM cortex_chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn bm25_tables_exist_after_migrate() {
        let conn = in_memory();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bm25_files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn sessions_table_exists_after_migrate() {
        let conn = in_memory();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn instinct_candidates_table_exists() {
        let conn = in_memory();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM instinct_candidates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn task_graph_tables_exist() {
        let conn = in_memory();
        let count_graphs: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_graphs", [], |r| r.get(0))
            .unwrap();
        let count_nodes: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_graph_nodes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_graphs, 0);
        assert_eq!(count_nodes, 0);
    }

    #[test]
    fn swarm_tasks_table_exists() {
        let conn = in_memory();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM swarm_tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn control_plane_tables_exist() {
        let conn = in_memory();
        let count_tasks: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_tasks", [], |r| r.get(0))
            .unwrap();
        let count_runs: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_agent_runs", [], |r| r.get(0))
            .unwrap();
        let count_artifacts: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_artifacts", [], |r| r.get(0))
            .unwrap();
        let count_approvals: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_approvals", [], |r| r.get(0))
            .unwrap();
        let count_edges: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_task_edges", [], |r| r.get(0))
            .unwrap();
        let count_graph_nodes: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_task_graph_nodes", [], |r| r.get(0))
            .unwrap();
        let count_graphs: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_task_graphs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_tasks, 0);
        assert_eq!(count_runs, 0);
        assert_eq!(count_artifacts, 0);
        assert_eq!(count_approvals, 0);
        assert_eq!(count_edges, 0);
        assert_eq!(count_graph_nodes, 0);
        assert_eq!(count_graphs, 0);
    }

    #[test]
    fn file_change_workflow_round_trip_applied() {
        let conn = in_memory();
        upsert_project(
            &conn, "RAIOS", "kernel", "/repo", None, "active", None, None, None, None,
        )
        .unwrap();

        let ids = create_file_change_workflow(&conn, "/repo/src/main.rs", "old", "new", "claude")
            .unwrap();
        mark_file_change_workflow_applied(&conn, &ids, "human").unwrap();

        let approval_status: String = conn
            .query_row(
                "SELECT status FROM cp_approvals WHERE id = ?1",
                params![ids.approval_id],
                |row| row.get(0),
            )
            .unwrap();
        let artifact_status: String = conn
            .query_row(
                "SELECT status FROM cp_artifacts WHERE id = ?1",
                params![ids.artifact_id],
                |row| row.get(0),
            )
            .unwrap();
        let task_status: String = conn
            .query_row(
                "SELECT status FROM cp_tasks WHERE id = ?1",
                params![ids.task_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(approval_status, "approved");
        assert_eq!(artifact_status, "applied");
        assert_eq!(task_status, "completed");
    }

    #[test]
    fn file_change_workflow_round_trip_rejected() {
        let conn = in_memory();
        let ids =
            create_file_change_workflow(&conn, "/tmp/notes.md", "old", "new", "gemini").unwrap();
        mark_file_change_workflow_rejected(&conn, &ids, "human", "rejected_by_user").unwrap();

        let run_status: String = conn
            .query_row(
                "SELECT status FROM cp_agent_runs WHERE id = ?1",
                params![ids.agent_run_id],
                |row| row.get(0),
            )
            .unwrap();
        let artifact_status: String = conn
            .query_row(
                "SELECT status FROM cp_artifacts WHERE id = ?1",
                params![ids.artifact_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(run_status, "failed");
        assert_eq!(artifact_status, "rejected");
    }

    // ── Canonical personal task tests ─────────────────────────────────────────

    #[test]
    fn cp_personal_tasks_insert_list_round_trip() {
        let conn = in_memory();
        let inputs = vec![
            PersonalTaskInput {
                id: None,
                title: "Write tests".into(),
                completed: false,
                agent: Some("claude".into()),
                project_name: Some("RAIOS".into()),
                display_order: 0,
            },
            PersonalTaskInput {
                id: None,
                title: "Deploy".into(),
                completed: true,
                agent: None,
                project_name: None,
                display_order: 1,
            },
        ];
        cp_sync_personal_tasks(&conn, &inputs, "/dev_ops/tasks.md").unwrap();
        let rows = cp_list_personal_tasks(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "Write tests");
        assert!(!rows[0].completed);
        assert_eq!(rows[0].assignee_id.as_deref(), Some("claude"));
        assert_eq!(rows[0].project_name.as_deref(), Some("RAIOS"));
        assert_eq!(rows[1].title, "Deploy");
        assert!(rows[1].completed);
    }

    #[test]
    fn cp_personal_tasks_cancel_on_sync() {
        let conn = in_memory();
        let inputs = vec![
            PersonalTaskInput {
                id: None,
                title: "Task A".into(),
                completed: false,
                agent: None,
                project_name: None,
                display_order: 0,
            },
            PersonalTaskInput {
                id: None,
                title: "Task B".into(),
                completed: false,
                agent: None,
                project_name: None,
                display_order: 1,
            },
        ];
        cp_sync_personal_tasks(&conn, &inputs, "/dev_ops/tasks.md").unwrap();
        assert_eq!(cp_list_personal_tasks(&conn).unwrap().len(), 2);

        // Re-sync with only Task A — Task B should be cancelled
        let keep_one = vec![PersonalTaskInput {
            id: None,
            title: "Task A".into(),
            completed: false,
            agent: None,
            project_name: None,
            display_order: 0,
        }];
        cp_sync_personal_tasks(&conn, &keep_one, "/dev_ops/tasks.md").unwrap();
        let rows = cp_list_personal_tasks(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Task A");

        let cancelled_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM cp_tasks WHERE status='cancelled' AND plan_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cancelled_count, 1);
    }

    #[test]
    fn cp_personal_tasks_title_dedup() {
        let conn = in_memory();
        let input = vec![PersonalTaskInput {
            id: None,
            title: "Do something".into(),
            completed: false,
            agent: None,
            project_name: None,
            display_order: 0,
        }];
        cp_sync_personal_tasks(&conn, &input, "/dev_ops/tasks.md").unwrap();
        cp_sync_personal_tasks(&conn, &input, "/dev_ops/tasks.md").unwrap();
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM cp_tasks WHERE plan_id IS NULL AND title='Do something'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, 1);
    }

    #[test]
    fn cp_rebuild_personal_markdown_creates_file() {
        let conn = in_memory();
        let dir = tempfile::tempdir().unwrap();
        let inputs = vec![
            PersonalTaskInput {
                id: None,
                title: "First task".into(),
                completed: false,
                agent: Some("claude".into()),
                project_name: None,
                display_order: 0,
            },
            PersonalTaskInput {
                id: None,
                title: "Second task".into(),
                completed: true,
                agent: None,
                project_name: Some("PROJ".into()),
                display_order: 1,
            },
        ];
        cp_sync_personal_tasks(&conn, &inputs, "/dev_ops/tasks.md").unwrap();
        cp_rebuild_personal_markdown(&conn, dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join("tasks.md")).unwrap();
        assert!(content.contains("- [ ] First task @claude"));
        assert!(content.contains("- [x] Second task #PROJ"));
    }

    #[test]
    fn cp_provider_capabilities_upsert_and_get() {
        let conn = in_memory();
        let caps = ProviderCapabilities {
            provider: "claude".into(),
            supports_tool_calling: true,
            supports_patch_diff: true,
            supports_long_running: true,
            supports_streaming: true,
            supports_exact_quota_visibility: false,
        };
        cp_upsert_provider_capabilities(&conn, &caps).unwrap();
        let got = cp_get_provider_capabilities(&conn, "claude").unwrap().unwrap();
        assert!(got.supports_tool_calling);
        assert!(got.supports_patch_diff);
        assert!(!got.supports_exact_quota_visibility);
        assert_eq!(got.provider, "claude");
    }

    #[test]
    fn cp_provider_capabilities_seed_and_list() {
        let conn = in_memory();
        cp_seed_provider_capabilities(&conn).unwrap();
        let all = cp_list_provider_capabilities(&conn).unwrap();
        assert!(all.len() >= 5);
        let names: Vec<&str> = all.iter().map(|c| c.provider.as_str()).collect();
        assert!(names.contains(&"claude"));
        assert!(names.contains(&"gemini"));
        assert!(names.contains(&"shell"));
    }

    #[test]
    fn cp_provider_capabilities_seed_no_overwrite() {
        let conn = in_memory();
        // First seed
        cp_seed_provider_capabilities(&conn).unwrap();
        // Manually override
        conn.execute(
            "UPDATE cp_provider_capabilities SET supports_tool_calling=0 WHERE provider='claude'",
            [],
        )
        .unwrap();
        // Re-seed should NOT overwrite
        cp_seed_provider_capabilities(&conn).unwrap();
        let got = cp_get_provider_capabilities(&conn, "claude").unwrap().unwrap();
        assert!(!got.supports_tool_calling, "seed should not overwrite existing rows");
    }

    #[test]
    fn cp_failure_kind_classify() {
        assert_eq!(ProviderFailureKind::classify("401 unauthorized"), ProviderFailureKind::Auth);
        assert_eq!(ProviderFailureKind::classify("rate limit exceeded 429"), ProviderFailureKind::Quota);
        assert_eq!(ProviderFailureKind::classify("connection timed out"), ProviderFailureKind::Timeout);
        assert_eq!(ProviderFailureKind::classify("sandbox permission denied"), ProviderFailureKind::Sandbox);
        assert_eq!(ProviderFailureKind::classify("invalid tool call argument"), ProviderFailureKind::ToolError);
        assert_eq!(ProviderFailureKind::classify("service unavailable 503"), ProviderFailureKind::ProviderUnavailable);
        assert!(matches!(ProviderFailureKind::classify("some weird error"), ProviderFailureKind::Unknown(_)));
    }

    #[test]
    fn cp_failure_kind_round_trip() {
        let kinds = [
            ProviderFailureKind::Auth,
            ProviderFailureKind::Quota,
            ProviderFailureKind::Timeout,
            ProviderFailureKind::Sandbox,
            ProviderFailureKind::ToolError,
            ProviderFailureKind::HumanRejection,
            ProviderFailureKind::ProviderUnavailable,
        ];
        for kind in &kinds {
            assert_eq!(&ProviderFailureKind::from_stored(kind.as_str()), kind);
        }
    }

    #[test]
    fn cp_route_to_capable_provider_prefers_claude() {
        let conn = in_memory();
        cp_seed_provider_capabilities(&conn).unwrap();
        let best = cp_route_to_capable_provider(&conn, true, false, false).unwrap();
        assert_eq!(best, Some("claude".to_string()));
    }

    #[test]
    fn cp_route_to_capable_provider_no_match() {
        let conn = in_memory();
        // Register only a provider that lacks tool calling
        cp_upsert_provider_capabilities(
            &conn,
            &ProviderCapabilities {
                provider: "basic".into(),
                supports_tool_calling: false,
                supports_patch_diff: false,
                supports_long_running: false,
                supports_streaming: false,
                supports_exact_quota_visibility: false,
            },
        )
        .unwrap();
        let best = cp_route_to_capable_provider(&conn, true, false, false).unwrap();
        assert_eq!(best, None);
    }

    // ── Integration: file approval lifecycle ────────────────────────────────────

    #[test]
    fn cp_flow_file_approval_lifecycle() {
        let conn = in_memory();
        let ids =
            create_file_change_workflow(&conn, "/proj/src/main.rs", "old", "new", "claude").unwrap();

        // Rows created in all four tables
        let task_status: String = conn
            .query_row("SELECT status FROM cp_tasks WHERE id=?1", params![ids.task_id], |r| r.get(0))
            .unwrap();
        assert_eq!(task_status, "awaiting_approval");

        let run_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_agent_runs WHERE task_id=?1", params![ids.task_id], |r| r.get(0))
            .unwrap();
        assert_eq!(run_count, 1);

        let artifact_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_artifacts WHERE task_id=?1", params![ids.task_id], |r| r.get(0))
            .unwrap();
        assert_eq!(artifact_count, 1);

        let approval_status: String = conn
            .query_row("SELECT status FROM cp_approvals WHERE id=?1", params![ids.approval_id], |r| r.get(0))
            .unwrap();
        assert_eq!(approval_status, "pending");

        // Run contract was persisted
        let contract_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM cp_run_contracts WHERE task_id=?1", params![ids.task_id], |r| r.get(0))
            .unwrap();
        assert_eq!(contract_count, 1);

        // Inbox query sees the pending approval
        let approvals = cp_query_pending_approvals(&conn).unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].approval_type, "file_write");

        // Approve: resolve the approval and mark task completed
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "UPDATE cp_approvals SET status='approved', resolved_at=?1 WHERE id=?2",
            params![now, ids.approval_id],
        )
        .unwrap();
        conn.execute(
            "UPDATE cp_tasks SET status='completed', updated_at=?1 WHERE id=?2",
            params![now, ids.task_id],
        )
        .unwrap();

        // No more pending approvals
        let after = cp_query_pending_approvals(&conn).unwrap();
        assert_eq!(after.len(), 0);

        // Active tasks no longer includes the completed task
        let active = cp_query_active_tasks(&conn).unwrap();
        assert!(!active.iter().any(|t| t.id == ids.task_id));
    }

    // ── Integration: swarm lifecycle ────────────────────────────────────────────

    #[test]
    fn cp_flow_swarm_lifecycle() {
        let conn = in_memory();
        let ids = create_swarm_workflow(&conn, "/proj", "add dark mode", "claude").unwrap();

        // Task starts as 'queued', run as 'pending'
        let task_status: String = conn
            .query_row("SELECT status FROM cp_tasks WHERE id=?1", params![ids.task_id], |r| r.get(0))
            .unwrap();
        assert_eq!(task_status, "queued");

        // Mark running
        mark_swarm_workflow_running(&conn, &ids.task_id, &ids.agent_run_id).unwrap();
        let task_status_2: String = conn
            .query_row("SELECT status FROM cp_tasks WHERE id=?1", params![ids.task_id], |r| r.get(0))
            .unwrap();
        assert_eq!(task_status_2, "running");

        // Active runs query shows the run
        let runs = cp_query_active_runs(&conn).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].provider, "swarm");
        assert_eq!(runs[0].task_id, ids.task_id);

        // Record a failure
        cp_record_run_failure(
            &conn,
            &ids.agent_run_id,
            &ProviderFailureKind::Timeout,
            "exceeded 3600s",
        )
        .unwrap();

        let exit_reason: String = conn
            .query_row("SELECT exit_reason FROM cp_agent_runs WHERE id=?1", params![ids.agent_run_id], |r| r.get(0))
            .unwrap();
        assert_eq!(exit_reason, "timeout");

        // No more active runs after failure
        let runs_after = cp_query_active_runs(&conn).unwrap();
        assert_eq!(runs_after.len(), 0);
    }

    // ── Integration: task graph lifecycle ───────────────────────────────────────

    #[test]
    fn cp_flow_task_graph_lifecycle() {
        let conn = in_memory();
        let graph_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        // task_graphs is the FK parent; cp_task_graph_nodes references it
        conn.execute(
            "INSERT INTO task_graphs (id, goal, agent, status, created_at) VALUES (?1,'goal','claude','pending',?2)",
            params![graph_id, now],
        )
        .unwrap();

        // Node A — ready immediately
        let ids_a = create_task_graph_node_workflow(
            &conn, &graph_id, "a", "build", "cargo build", "claude", true,
        )
        .unwrap();

        // Node B — queued (depends on A)
        let ids_b = create_task_graph_node_workflow(
            &conn, &graph_id, "b", "test", "cargo test", "claude", false,
        )
        .unwrap();

        // Scheduler sees node A as ready
        let ready = cp_scheduler_list_ready(&conn).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, ids_a.task_id);
        assert_eq!(ready[0].execution_kind, "task_graph");

        // Shell cmd is retrievable
        let cmd = cp_task_graph_shell_cmd(&conn, &ids_a.task_id).unwrap();
        assert_eq!(cmd.as_deref(), Some("cargo build"));

        // Complete node A, promote node B to ready
        let completed_at = now.clone();
        conn.execute(
            "UPDATE cp_tasks SET status='completed', updated_at=?1 WHERE id=?2",
            params![completed_at, ids_a.task_id],
        )
        .unwrap();
        conn.execute(
            "UPDATE cp_tasks SET status='ready', updated_at=?1 WHERE id=?2",
            params![now, ids_b.task_id],
        )
        .unwrap();

        let ready2 = cp_scheduler_list_ready(&conn).unwrap();
        assert_eq!(ready2.len(), 1);
        assert_eq!(ready2[0].id, ids_b.task_id);
    }

    // ── Integration: personal task lifecycle ────────────────────────────────────

    #[test]
    fn cp_flow_personal_task_lifecycle() {
        let conn = in_memory();

        // Create two tasks
        let inputs = vec![
            PersonalTaskInput {
                id: None,
                title: "Write tests".into(),
                completed: false,
                agent: None,
                project_name: Some("RAIOS".into()),
                display_order: 0,
            },
            PersonalTaskInput {
                id: None,
                title: "Update docs".into(),
                completed: false,
                agent: Some("claude".into()),
                project_name: None,
                display_order: 1,
            },
        ];
        cp_sync_personal_tasks(&conn, &inputs, "/dev_ops/tasks.md").unwrap();
        let rows = cp_list_personal_tasks(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].completed);

        // Toggle first task to completed
        let first_id = rows[0].id.clone();
        let updated = vec![
            PersonalTaskInput {
                id: Some(first_id.clone()),
                title: rows[0].title.clone(),
                completed: true,
                agent: rows[0].assignee_id.clone(),
                project_name: rows[0].project_name.clone(),
                display_order: 0,
            },
            PersonalTaskInput {
                id: Some(rows[1].id.clone()),
                title: rows[1].title.clone(),
                completed: false,
                agent: rows[1].assignee_id.clone(),
                project_name: rows[1].project_name.clone(),
                display_order: 1,
            },
        ];
        cp_sync_personal_tasks(&conn, &updated, "/dev_ops/tasks.md").unwrap();

        let after = cp_list_personal_tasks(&conn).unwrap();
        let first = after.iter().find(|r| r.id == first_id).unwrap();
        assert!(first.completed);

        // Rebuild markdown
        let dir = tempfile::tempdir().unwrap();
        cp_rebuild_personal_markdown(&conn, dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join("tasks.md")).unwrap();
        assert!(content.contains("- [x]"));
        assert!(content.contains("- [ ]"));
    }

    // ── Integration: unified inbox view ────────────────────────────────────────

    #[test]
    fn cp_flow_unified_inbox() {
        let conn = in_memory();

        // File approval task
        create_file_change_workflow(&conn, "/proj/a.rs", "old", "new", "claude").unwrap();

        // Swarm task
        create_swarm_workflow(&conn, "/proj", "dark mode", "claude").unwrap();

        // Personal task
        cp_sync_personal_tasks(
            &conn,
            &[PersonalTaskInput {
                id: None,
                title: "Personal work".into(),
                completed: false,
                agent: None,
                project_name: None,
                display_order: 0,
            }],
            "/dev_ops/tasks.md",
        )
        .unwrap();

        let snapshot = cp_daemon_snapshot(&conn).unwrap();

        // All 3 tasks show up as active
        assert_eq!(snapshot.active_tasks.len(), 3);

        // 2 agent runs (file-change + swarm; personal has no run)
        assert_eq!(snapshot.active_runs.len(), 2);

        // 1 pending file_write approval
        assert_eq!(snapshot.pending_approvals.len(), 1);
        assert_eq!(snapshot.pending_approvals[0].approval_type, "file_write");

        // No blocked tasks
        assert_eq!(snapshot.blocked_tasks.len(), 0);

        // Origins cover both swarm and file_approval
        let origins: Vec<&str> = snapshot.active_tasks.iter().map(|t| t.origin.as_str()).collect();
        assert!(origins.contains(&"swarm"));
        assert!(origins.contains(&"file_approval"));
        assert!(origins.contains(&"personal"));
    }

    // ── Integration: budget deferral appears in snapshot ────────────────────────

    #[test]
    fn cp_snapshot_budget_deferral_visible() {
        let conn = in_memory();
        // Exhaust the claude provider budget
        cp_upsert_budget_ledger(
            &conn,
            "provider",
            "claude",
            Some("claude"),
            "tokens",
            Some(10000.0),
            Some(10000.0),
            Some(0.0),
            "exact",
            "test",
        )
        .unwrap();
        let snap = cp_daemon_snapshot(&conn).unwrap();
        assert!(snap.budget_deferrals.contains(&"claude".to_string()));
    }

    // ── Integration: pending file-change approvals survive simulated restart ────

    #[test]
    fn cp_pending_file_change_approvals_hydrated_from_db() {
        let conn = in_memory();
        // Create a workflow — this stores everything in canonical tables
        let ids =
            create_file_change_workflow(&conn, "/proj/src/lib.rs", "old code", "new code", "claude")
                .unwrap();

        // Simulate daemon state reload from DB (what refresh_pending_from_db does)
        let rows = cp_load_pending_file_change_approvals(&conn).unwrap();
        assert_eq!(rows.len(), 1);

        let row = &rows[0];
        // Canonical ID matches what the workflow inserted into cp_approvals
        assert_eq!(row.approval_id, ids.approval_id);
        // Path and content are hydrated from cp_artifacts.metadata_json
        assert_eq!(row.path, "/proj/src/lib.rs");
        assert_eq!(row.original_content, "old code");
        assert_eq!(row.new_content, "new code");
        assert_eq!(row.agent_name, "claude");
        // FK chains are intact
        assert_eq!(row.task_id.as_deref(), Some(ids.task_id.as_str()));
        assert_eq!(row.agent_run_id.as_deref(), Some(ids.agent_run_id.as_str()));
        assert_eq!(row.artifact_id.as_deref(), Some(ids.artifact_id.as_str()));
    }

    #[test]
    fn cp_pending_file_change_approvals_disappears_after_resolve() {
        let conn = in_memory();
        let ids =
            create_file_change_workflow(&conn, "/proj/src/main.rs", "v1", "v2", "gemini").unwrap();

        // Pre-condition: visible
        assert_eq!(cp_load_pending_file_change_approvals(&conn).unwrap().len(), 1);

        // Approve via canonical DB update
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "UPDATE cp_approvals SET status='approved', resolved_at=?1 WHERE id=?2",
            rusqlite::params![now, ids.approval_id],
        )
        .unwrap();

        // Post-condition: no longer pending
        assert_eq!(cp_load_pending_file_change_approvals(&conn).unwrap().len(), 0);
    }
}
