use rusqlite::{params, Connection, Result};
use std::path::Path;

pub mod agent_stats;
pub mod budget_gate;
pub mod control_plane;
pub mod factory_cycles;
pub mod factory_evidence;
pub mod factory_intake;
pub mod factory_policies;
pub mod factory_products;
pub mod factory_releases;
pub mod factory_requirements;
pub mod factory_support;
pub mod health;
pub mod inbox_risk;
pub mod mem;
pub mod projects;
pub mod provider;
pub mod run_contract;
pub mod scheduler;
mod schema;
pub mod tasks;
pub mod tool_traces;
pub mod wf_file_change;
pub mod wf_handoff;
pub mod wf_sessions;
pub mod wf_swarm;
pub mod wf_task_graph;

pub use agent_stats::*;
pub use budget_gate::*;
pub use control_plane::*;
pub use factory_cycles::*;
pub use factory_evidence::*;
pub use factory_intake::*;
pub use factory_policies::*;
pub use factory_products::*;
pub use factory_releases::*;
pub use factory_requirements::*;
pub use factory_support::*;
pub use health::*;
pub use inbox_risk::*;
pub use mem::*;
pub use projects::*;
pub use provider::*;
pub use run_contract::*;
pub use scheduler::*;
pub use tasks::*;
pub use tool_traces::*;
pub use wf_file_change::*;
pub use wf_handoff::*;
pub use wf_sessions::*;
pub use wf_swarm::*;
pub use wf_task_graph::*;

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
    let mut stmt =
        conn.prepare("SELECT ts, sender, content FROM cp_logs ORDER BY id DESC LIMIT ?1")?;
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
    schema::migrate(conn)
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
