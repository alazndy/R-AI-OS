//! Project registration: makes an existing directory launchable and handoff-addressable.

use std::path::Path;

use raios_contracts::Problem;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use super::service::validate_project_path;

/// Registers an existing directory under its canonical path. An already-registered path is
/// reported and left untouched, so existing names and categories are never overwritten.
pub(crate) fn register_project(tx: &Connection, project_path: &str) -> Result<Value, Problem> {
    let requested = validate_project_path(Some(project_path))?
        .ok_or_else(|| Problem::invalid_input("Project path is required"))?;
    let canonical = Path::new(&requested)
        .canonicalize()
        .map_err(|_| Problem::not_found("Project directory does not exist"))?;
    if !canonical.is_dir() {
        return Err(Problem::invalid_input("Project path must be a directory"));
    }
    let name = canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| Problem::invalid_input("Project path must not be the filesystem root"))?;
    let canonical = canonical.to_string_lossy().into_owned();

    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM projects WHERE path = ?1",
            params![canonical],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| Problem::internal(error.to_string()))?;
    if existing.is_some() {
        return Ok(json!({"status": "already_registered", "project_path": canonical}));
    }

    tx.execute(
        "INSERT INTO projects (name, path) VALUES (?1, ?2)",
        params![name, canonical],
    )
    .map_err(|error| Problem::internal(error.to_string()))?;
    Ok(json!({"status": "registered", "project_path": canonical}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::service::{dispatch_control_command, ControlActor};
    use raios_contracts::Command;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        conn
    }

    fn register(conn: &mut Connection, key: &str, path: &str) -> Result<Option<Value>, Problem> {
        dispatch_control_command(
            conn,
            &ControlActor::local_session(),
            &Command::RegisterProject {
                project_path: path.into(),
                idempotency_key: key.into(),
            },
        )
    }

    fn row(conn: &Connection, path: &str) -> Option<(String, String)> {
        conn.query_row(
            "SELECT name, category FROM projects WHERE path = ?1",
            [path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
    }

    #[test]
    fn registers_an_existing_directory_under_its_directory_name() {
        let mut conn = db();
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("my-app");
        std::fs::create_dir(&project).unwrap();
        let canonical = project.canonicalize().unwrap();

        let result = register(&mut conn, "reg-1", project.to_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(result["status"], "registered");
        assert_eq!(result["project_path"], canonical.to_str().unwrap());
        assert_eq!(row(&conn, canonical.to_str().unwrap()).unwrap().0, "my-app");
        assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 1);
    }

    #[test]
    fn re_registering_preserves_existing_metadata() {
        let mut conn = db();
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("kept");
        std::fs::create_dir(&project).unwrap();
        let canonical = project.canonicalize().unwrap();
        let canonical = canonical.to_str().unwrap();
        conn.execute(
            "INSERT INTO projects (name, path, category) VALUES ('Keep Me', ?1, 'kernel')",
            [canonical],
        )
        .unwrap();

        let result = register(&mut conn, "reg-2", canonical).unwrap().unwrap();
        assert_eq!(result["status"], "already_registered");
        assert_eq!(
            row(&conn, canonical).unwrap(),
            ("Keep Me".to_string(), "kernel".to_string())
        );
    }

    #[test]
    fn rejects_relative_missing_file_and_root_paths() {
        let mut conn = db();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        let missing = dir.path().join("does-not-exist");

        let cases = [
            ("bad-1", "relative/path".to_string(), "INVALID_INPUT"),
            ("bad-2", missing.to_str().unwrap().to_string(), "NOT_FOUND"),
            ("bad-3", file.to_str().unwrap().to_string(), "INVALID_INPUT"),
            ("bad-4", "/".to_string(), "INVALID_INPUT"),
        ];
        for (key, path, code) in cases {
            let error = register(&mut conn, key, &path).unwrap_err();
            assert_eq!(error.code, code, "{path}");
        }
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_registers_its_target_not_the_link() {
        let mut conn = db();
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real-project");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link-to-project");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canonical = real.canonicalize().unwrap();

        let result = register(&mut conn, "reg-3", link.to_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(result["project_path"], canonical.to_str().unwrap());
        assert!(row(&conn, link.to_str().unwrap()).is_none());
        assert!(row(&conn, canonical.to_str().unwrap()).is_some());
    }
}
