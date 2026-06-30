use super::*;
use rusqlite::{params, Connection, OptionalExtension, Result};
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
             status=CASE WHEN status IN ('beklemede','archived','waiting') THEN status WHEN excluded.status IN ('production','active','early','legacy','waiting') THEN excluded.status ELSE status END,
             stars=COALESCE(excluded.stars, stars),
             last_commit=COALESCE(excluded.last_commit, last_commit),
             version=COALESCE(excluded.version, version),
             nickname=COALESCE(excluded.nickname, nickname),
             updated_at=datetime('now')",
        params![name, category, path, github, status, stars, last_commit, version, nickname],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Directly update a project's lifecycle status by path.
/// Used by the lifecycle worker — bypasses the upsert preserve logic.
pub fn update_project_status(conn: &Connection, path: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE projects SET status = ?1, updated_at = datetime('now') WHERE path = ?2",
        params![status, path],
    )?;
    Ok(())
}

pub fn project_id_for_path(conn: &Connection, path: &str) -> Option<i64> {
    conn.query_row(
        "SELECT id FROM projects WHERE path = ?1",
        params![path],
        |row| row.get(0),
    )
    .ok()
}

