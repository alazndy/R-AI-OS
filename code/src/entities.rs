use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

// ─── Public struct (unchanged API) ───────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntityProject {
    pub name: String,
    pub category: String,
    pub local_path: PathBuf,
    pub github: Option<String>,
    pub status: String,
    pub stars: Option<u32>,
    pub last_commit: Option<String>,
    pub version: Option<String>,
    pub version_nickname: Option<String>,
}

// ─── Load ─────────────────────────────────────────────────────────────────────

pub fn load_entities(dev_ops: &Path) -> Vec<EntityProject> {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(_) => return load_entities_json_fallback(dev_ops),
    };

    // One-time migration from entities.json
    crate::db::import_from_json(dev_ops, &conn);

    match crate::db::load_all_projects(&conn) {
        Ok(rows) => rows.into_iter()
            .filter(|r| Path::new(&r.path).exists())
            .map(row_to_entity)
            .collect(),
        Err(_) => vec![],
    }
}

// ─── Save ─────────────────────────────────────────────────────────────────────

pub fn save_entities(_dev_ops: &Path, projects: Vec<EntityProject>) -> std::io::Result<()> {
    let conn = crate::db::open_db()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    for p in &projects {
        let path_str = p.local_path.to_string_lossy().to_string();
        crate::db::upsert_project(
            &conn,
            &p.name,
            &p.category,
            &path_str,
            p.github.as_deref(),
            &p.status,
            p.stars.map(|s| s as i64),
            p.last_commit.as_deref(),
            p.version.as_deref(),
            p.version_nickname.as_deref(),
        ).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    }
    Ok(())
}

// ─── Discover (scanner → SQLite merge) ───────────────────────────────────────

pub fn discover_entities(dev_ops: &Path) -> Vec<EntityProject> {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    // One-time migration
    crate::db::import_from_json(dev_ops, &conn);

    // Scan workspace
    let rooms = crate::mempalace::build(dev_ops);
    for room in rooms {
        for proj in room.projects {
            if !proj.path.exists() { continue; }
            let path_str = proj.path.to_string_lossy().to_string();
            let _ = crate::db::upsert_project(
                &conn,
                &proj.name,
                &room.folder_name,
                &path_str,
                None,
                &proj.status,
                None,
                None,
                proj.version.as_deref(),
                proj.version_nickname.as_deref(),
            );
        }
    }

    // Return everything that still exists on disk
    match crate::db::load_all_projects(&conn) {
        Ok(rows) => rows.into_iter()
            .filter(|r| Path::new(&r.path).exists())
            .map(row_to_entity)
            .collect(),
        Err(_) => vec![],
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn row_to_entity(r: crate::db::DbProject) -> EntityProject {
    EntityProject {
        name:             r.name,
        category:         r.category,
        local_path:       PathBuf::from(&r.path),
        github:           r.github,
        status:           r.status,
        stars:            r.stars.map(|s| s as u32),
        last_commit:      r.last_commit,
        version:          r.version,
        version_nickname: r.nickname,
    }
}

/// Fallback: read old entities.json (used if SQLite unavailable)
fn load_entities_json_fallback(dev_ops: &Path) -> Vec<EntityProject> {
    #[derive(Deserialize)]
    struct EntitiesFile {
        #[serde(default)]
        projects: Vec<EntityProject>,
    }
    let path = dev_ops.join("entities.json");
    let content = std::fs::read_to_string(path).unwrap_or_default();
    serde_json::from_str::<EntitiesFile>(&content)
        .map(|f| f.projects)
        .unwrap_or_default()
}
