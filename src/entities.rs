use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

    let projects = match crate::db::load_all_projects(&conn) {
        Ok(rows) => rows
            .into_iter()
            .filter(|r| Path::new(&r.path).exists())
            .map(row_to_entity)
            .collect(),
        Err(_) => vec![],
    };
    dedup_nested(projects)
}

// ─── Save ─────────────────────────────────────────────────────────────────────

pub fn save_entities(_dev_ops: &Path, projects: Vec<EntityProject>) -> std::io::Result<()> {
    let conn = crate::db::open_db().map_err(std::io::Error::other)?;

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
        )
        .map_err(std::io::Error::other)?;
    }
    Ok(())
}

// ─── Discover (scanner → SQLite merge) ───────────────────────────────────────

pub fn discover_entities(dev_ops: &Path) -> Vec<EntityProject> {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    // One-time migration from entities.json (runs only once)
    crate::db::import_from_json(dev_ops, &conn);

    // Fresh scan — collect only what exists on disk right now
    let rooms = crate::mempalace::build(dev_ops);
    let mut fresh_paths: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for room in &rooms {
        for proj in &room.projects {
            if !proj.path.exists() {
                continue;
            }
            // Only track projects that have a memory.md — untracked dirs are ignored
            if !proj.path.join("memory.md").exists() {
                continue;
            }
            let path_str = proj.path.to_string_lossy().to_string();
            fresh_paths.insert(path_str.clone());
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

    // Return ONLY freshly scanned projects — ignore stale DB accumulation
    let projects = match crate::db::load_all_projects(&conn) {
        Ok(rows) => rows
            .into_iter()
            .filter(|r| fresh_paths.contains(&r.path) && Path::new(&r.path).exists())
            .map(row_to_entity)
            .collect(),
        Err(_) => vec![],
    };
    dedup_nested(projects)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn row_to_entity(r: crate::db::DbProject) -> EntityProject {
    EntityProject {
        name: r.name,
        category: r.category,
        local_path: PathBuf::from(&r.path),
        github: r.github,
        status: r.status,
        stars: r.stars.map(|s| s as u32),
        last_commit: r.last_commit,
        version: r.version,
        version_nickname: r.nickname,
    }
}

/// Remove projects whose path is a sub-directory of another project in the list.
/// Sorts by path depth (shallowest first), accepts a project only if no already-accepted
/// project is an ancestor of it. Also removes canonical-path duplicates (symlinks).
fn dedup_nested(mut projects: Vec<EntityProject>) -> Vec<EntityProject> {
    // Resolve canonical paths; drop entries we cannot canonicalize
    let mut canonical: Vec<(PathBuf, EntityProject)> = projects
        .drain(..)
        .filter_map(|p| {
            p.local_path
                .canonicalize()
                .ok()
                .map(|canon| (canon, p))
        })
        .collect();

    // Sort shallowest path first so parents are accepted before children
    canonical.sort_by_key(|(canon, _)| canon.components().count());

    let mut seen_paths: Vec<PathBuf> = Vec::new();
    let mut result: Vec<EntityProject> = Vec::new();

    for (canon, proj) in canonical {
        // Skip if this path is nested inside an already-accepted project
        let is_nested = seen_paths.iter().any(|accepted| canon.starts_with(accepted));
        if !is_nested {
            seen_paths.push(canon);
            result.push(proj);
        }
    }
    result
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
