use std::path::Path;
use rusqlite::{Connection, Result, params};

// ─── Open & migrate ──────────────────────────────────────────────────────────

pub fn open_db() -> Result<Connection> {
    let db_path = db_path();
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(&db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

fn db_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("raios")
        .join("workspace.db")
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("
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
            remote_url       TEXT,
            scanned_at       TEXT DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS tasks (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            text       TEXT NOT NULL,
            completed  INTEGER NOT NULL DEFAULT 0,
            agent      TEXT,
            project    TEXT,
            created_at TEXT DEFAULT (datetime('now'))
        );
    ")?;
    Ok(())
}

// ─── Migration from entities.json ────────────────────────────────────────────

/// One-time import from entities.json → SQLite. Deletes json after success.
pub fn import_from_json(dev_ops: &Path, conn: &Connection) -> usize {
    let json_path = dev_ops.join("entities.json");
    if !json_path.exists() { return 0; }

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
    fn default_status() -> String { "active".into() }

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
        if !p.local_path.exists() { continue; }
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
        if result.is_ok() { imported += 1; }
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
         FROM projects ORDER BY name"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(DbProject {
            id:          row.get(0)?,
            name:        row.get(1)?,
            category:    row.get(2)?,
            path:        row.get(3)?,
            github:      row.get(4)?,
            status:      row.get(5)?,
            stars:       row.get(6)?,
            last_commit: row.get(7)?,
            version:     row.get(8)?,
            nickname:    row.get(9)?,
        })
    })?;
    rows.collect()
}

pub fn upsert_project(conn: &Connection, name: &str, category: &str, path: &str,
    github: Option<&str>, status: &str, stars: Option<i64>,
    last_commit: Option<&str>, version: Option<&str>, nickname: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO projects (name, category, path, github, status, stars, last_commit, version, nickname)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(path) DO UPDATE SET
             name=excluded.name, category=excluded.category,
             github=COALESCE(excluded.github, github),
             status=excluded.status,
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
    ).ok()
}

// ─── Health cache ─────────────────────────────────────────────────────────────

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
    remote_url: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO health_cache
            (project_id, compliance_grade, compliance_score, security_grade, security_score,
             security_issues, security_critical, git_dirty, has_memory, remote_url, scanned_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'))
         ON CONFLICT(project_id) DO UPDATE SET
             compliance_grade=excluded.compliance_grade,
             compliance_score=excluded.compliance_score,
             security_grade=COALESCE(excluded.security_grade, security_grade),
             security_score=COALESCE(excluded.security_score, security_score),
             security_issues=excluded.security_issues,
             security_critical=excluded.security_critical,
             git_dirty=excluded.git_dirty,
             has_memory=excluded.has_memory,
             remote_url=COALESCE(excluded.remote_url, remote_url),
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
            remote_url,
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
    let active: i64 = conn.query_row("SELECT COUNT(*) FROM projects WHERE status = 'active'", [], |r| r.get(0))?;
    let archived: i64 = conn.query_row("SELECT COUNT(*) FROM projects WHERE status IN ('archived','legacy')", [], |r| r.get(0))?;
    let dirty: i64 = conn.query_row("SELECT COUNT(*) FROM health_cache WHERE git_dirty = 1", [], |r| r.get(0))?;
    let no_memory: i64 = conn.query_row("SELECT COUNT(*) FROM health_cache WHERE has_memory = 0", [], |r| r.get(0))?;
    let no_github: i64 = conn.query_row("SELECT COUNT(*) FROM projects WHERE github IS NULL", [], |r| r.get(0))?;
    let avg_compliance: f64 = conn.query_row("SELECT COALESCE(AVG(compliance_score), 0) FROM health_cache", [], |r| r.get(0))?;
    let avg_security: f64 = conn.query_row("SELECT COALESCE(AVG(security_score), 0) FROM health_cache", [], |r| r.get(0))?;
    let grade_a: i64 = conn.query_row("SELECT COUNT(*) FROM health_cache WHERE compliance_grade = 'A'", [], |r| r.get(0))?;
    let grade_b: i64 = conn.query_row("SELECT COUNT(*) FROM health_cache WHERE compliance_grade = 'B'", [], |r| r.get(0))?;
    let grade_c: i64 = conn.query_row("SELECT COUNT(*) FROM health_cache WHERE compliance_grade = 'C'", [], |r| r.get(0))?;
    let grade_d: i64 = conn.query_row("SELECT COUNT(*) FROM health_cache WHERE compliance_grade NOT IN ('A','B','C') AND compliance_grade != '-'", [], |r| r.get(0))?;

    Ok(PortfolioStats { total, active, archived, dirty, no_memory, no_github,
        avg_compliance, avg_security, grade_a, grade_b, grade_c, grade_d })
}

// ─── Tasks ───────────────────────────────────────────────────────────────────

pub struct DbTask {
    pub id: i64,
    pub text: String,
    pub completed: bool,
    pub agent: Option<String>,
    pub project: Option<String>,
}

pub fn load_tasks_db(conn: &Connection) -> Result<Vec<DbTask>> {
    let mut stmt = conn.prepare(
        "SELECT id, text, completed, agent, project FROM tasks ORDER BY id"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(DbTask {
            id:        row.get(0)?,
            text:      row.get(1)?,
            completed: row.get::<_, i64>(2)? != 0,
            agent:     row.get(3)?,
            project:   row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn insert_task(conn: &Connection, text: &str, agent: Option<&str>, project: Option<&str>) -> Result<i64> {
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
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn project_crud_round_trip() {
        let conn = in_memory();
        upsert_project(&conn, "TestProj", "devtools", "/tmp/test", None, "active", None, None, None, None).unwrap();
        let projects = load_all_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "TestProj");
    }

    #[test]
    fn upsert_is_idempotent() {
        let conn = in_memory();
        upsert_project(&conn, "P", "cat", "/tmp/p", Some("gh/p"), "active", None, None, None, None).unwrap();
        upsert_project(&conn, "P-renamed", "cat", "/tmp/p", None, "active", None, None, None, None).unwrap();
        let projects = load_all_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "P-renamed");
        assert_eq!(projects[0].github.as_deref(), Some("gh/p")); // preserved
    }

    #[test]
    fn health_cache_upsert() {
        let conn = in_memory();
        upsert_project(&conn, "P", "c", "/tmp/p", None, "active", None, None, None, None).unwrap();
        let id = project_id_for_path(&conn, "/tmp/p").unwrap();
        upsert_health(&conn, id, "A", Some(90), Some("A"), Some(95), 0, 0, false, true, None).unwrap();
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
}
