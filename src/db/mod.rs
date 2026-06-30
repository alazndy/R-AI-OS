use rusqlite::{params, Connection, OptionalExtension, Result};
use std::path::Path;

pub mod budget;
pub mod control_plane;
pub mod health;
pub mod mem;
pub mod projects;
pub mod scheduler;
pub mod tasks;
pub mod workflows;

pub use budget::*;
pub use control_plane::*;
pub use health::*;
pub use mem::*;
pub use projects::*;
pub use scheduler::*;
pub use tasks::*;
pub use workflows::*;

#[cfg(test)]
mod tests;

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
        "UPDATE projects SET status = 'active' WHERE status NOT IN ('production','active','early','legacy','waiting','beklemede','archived')",
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

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS cp_scheduled_jobs (
            id               TEXT PRIMARY KEY,
            title            TEXT NOT NULL,
            agent            TEXT NOT NULL,
            task_description TEXT NOT NULL,
            project_id       TEXT,
            interval_secs    INTEGER NOT NULL,
            status           TEXT NOT NULL DEFAULT 'active',
            last_run_at      TEXT,
            next_run_at      TEXT NOT NULL,
            created_at       TEXT NOT NULL,
            run_count        INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_cp_scheduled_jobs_status   ON cp_scheduled_jobs(status);
        CREATE INDEX IF NOT EXISTS idx_cp_scheduled_jobs_next_run ON cp_scheduled_jobs(next_run_at);
        ",
    )?;

    // ─── Log Ring Buffer ─────────────────────────────────────────────────────
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS cp_logs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            ts          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            sender      TEXT    NOT NULL,
            content     TEXT    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cp_logs_ts ON cp_logs(id DESC);
        ",
    )?;

    // ─── Agent Memory Items ───────────────────────────────────────────────────
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS mem_items (
            id          TEXT PRIMARY KEY,
            project_key TEXT NOT NULL,
            item_type   TEXT NOT NULL CHECK(item_type IN ('user','feedback','project','reference')),
            slug        TEXT NOT NULL,
            title       TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            body        TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
            session_id  TEXT,
            UNIQUE(project_key, slug)
        );
        CREATE INDEX IF NOT EXISTS idx_mem_items_project ON mem_items(project_key);
        CREATE INDEX IF NOT EXISTS idx_mem_items_type    ON mem_items(project_key, item_type);
        ",
    )?;

    Ok(())
}

// ─── cp_logs helpers ─────────────────────────────────────────────────────────

const LOG_RING_MAX: usize = 2000;

/// Append a log entry and prune oldest rows above the ring limit.
pub fn cp_log_append(conn: &Connection, sender: &str, content: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO cp_logs (sender, content) VALUES (?1, ?2)",
        params![sender, content],
    )?;
    // Prune oldest entries so table never exceeds LOG_RING_MAX rows
    conn.execute(
        "DELETE FROM cp_logs WHERE id <= (
            SELECT id FROM cp_logs ORDER BY id DESC LIMIT 1 OFFSET ?1
        )",
        params![LOG_RING_MAX as i64],
    )?;
    Ok(())
}

/// Return the last `limit` log entries in chronological order.
pub fn cp_logs_replay(conn: &Connection, limit: usize) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT ts, sender, content FROM cp_logs ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut entries: Vec<(String, String, String)> = rows.flatten().collect();
    entries.reverse(); // oldest first
    Ok(entries)
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

