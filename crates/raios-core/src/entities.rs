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
    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(_) => return load_entities_json_fallback(dev_ops),
    };

    // One-time migration from entities.json
    raios_core::db::import_from_json(dev_ops, &conn);

    let projects = match raios_core::db::load_all_projects(&conn) {
        Ok(rows) => rows
            .into_iter()
            .filter(|r| {
                Path::new(&r.path).exists() && r.status != "waiting" && r.status != "beklemede"
            })
            .map(row_to_entity)
            .collect(),
        Err(_) => vec![],
    };
    dedup_nested(projects)
}

// ─── Save ─────────────────────────────────────────────────────────────────────

pub fn save_entities(_dev_ops: &Path, projects: Vec<EntityProject>) -> std::io::Result<()> {
    let conn = raios_core::db::open_db().map_err(std::io::Error::other)?;

    for p in &projects {
        let path_str = p.local_path.to_string_lossy().to_string();
        raios_core::db::upsert_project(
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
    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    // One-time migration from entities.json (runs only once)
    raios_core::db::import_from_json(dev_ops, &conn);

    // Fresh scan — collect only what exists on disk right now
    let rooms = raios_core::mempalace::build(dev_ops);
    let mut fresh_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

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
            let _ = raios_core::db::upsert_project(
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
    let projects = match raios_core::db::load_all_projects(&conn) {
        Ok(rows) => rows
            .into_iter()
            .filter(|r| {
                fresh_paths.contains(&r.path)
                    && Path::new(&r.path).exists()
                    && r.status != "waiting"
                    && r.status != "beklemede"
            })
            .map(row_to_entity)
            .collect(),
        Err(_) => vec![],
    };
    dedup_nested(projects)
}

/// Pure filesystem scan — every project with a `memory.md`, no DB round-trip,
/// no status filtering. Unlike `load_entities`/`discover_entities`, this does
/// NOT exclude `waiting`/`beklemede` projects: callers that need the complete,
/// unfiltered project list (e.g. the Obsidian vault sync) use this instead.
pub fn discover_all_entities(dev_ops: &Path) -> Vec<EntityProject> {
    let rooms = raios_core::mempalace::build(dev_ops);
    let mut projects = Vec::new();

    for room in &rooms {
        for proj in &room.projects {
            if !proj.path.exists() || !proj.path.join("memory.md").exists() {
                continue;
            }
            projects.push(EntityProject {
                name: proj.name.clone(),
                category: room.folder_name.clone(),
                local_path: proj.path.clone(),
                github: None,
                status: proj.status.clone(),
                stars: None,
                last_commit: None,
                version: proj.version.clone(),
                version_nickname: proj.version_nickname.clone(),
            });
        }
    }

    dedup_nested(projects)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn row_to_entity(r: raios_core::db::DbProject) -> EntityProject {
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
        .filter_map(|p| p.local_path.canonicalize().ok().map(|canon| (canon, p)))
        .collect();

    // Sort shallowest path first so parents are accepted before children
    canonical.sort_by_key(|(canon, _)| canon.components().count());

    let mut seen_paths: Vec<PathBuf> = Vec::new();
    let mut result: Vec<EntityProject> = Vec::new();

    for (canon, proj) in canonical {
        // Skip if this path is nested inside an already-accepted project
        let is_nested = seen_paths
            .iter()
            .any(|accepted| canon.starts_with(accepted));
        if !is_nested {
            seen_paths.push(canon);
            result.push(proj);
        }
    }
    result
}

/// Whether a project has a live sigmap context map.
///
/// Older sigmap versions (and the workflow AGENT_CONSTITUTION.md Sec 7
/// describes) write a standalone `SIGMAP.md`. The sigmap version installed
/// as of 2026-08-17 (v8.18.0+) no longer produces that file by default —
/// it writes multi-adapter signature blocks into `CLAUDE.md`/`AGENTS.md`/
/// `.github/copilot-instructions.md`/`.github/gemini-context.md` instead,
/// dropping `gen-context.config.json` as its init marker. Checking only
/// for `SIGMAP.md` made every project using the current sigmap read as
/// "missing" across `raios health`/`reflect`/`pre-flight`/the TUI, even
/// right after a real `sigmap` run. `gen-context.config.json` is the one
/// file sigmap itself creates that nothing else plausibly would, so it's
/// the reliable signal for the new output style — checking for
/// `AGENTS.md`/`CLAUDE.md` presence alone would false-positive on any
/// project that hand-writes those for unrelated reasons.
pub fn has_sigmap_context(path: &Path) -> bool {
    path.join("SIGMAP.md").exists() || path.join("gen-context.config.json").exists()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_all_entities_includes_every_project_regardless_of_status() {
        let dev_ops = tempfile::tempdir().expect("tempdir");

        let with_status = dev_ops.path().join("ai").join("has-status");
        std::fs::create_dir_all(with_status.join(".git")).unwrap();
        std::fs::write(
            with_status.join("memory.md"),
            "# Memory\n\n## Son Durum\n- Durum: active\n",
        )
        .unwrap();

        let without_status = dev_ops.path().join("ai").join("no-status");
        std::fs::create_dir_all(without_status.join(".git")).unwrap();
        std::fs::write(
            without_status.join("memory.md"),
            "# Memory\n\nJust some notes, no status line.\n",
        )
        .unwrap();

        let no_memory = dev_ops.path().join("ai").join("untracked-dir");
        std::fs::create_dir_all(&no_memory).unwrap();

        let projects = discover_all_entities(dev_ops.path());
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();

        assert!(names.contains(&"has-status"));
        assert!(names.contains(&"no-status"));
        assert!(
            !names.contains(&"untracked-dir"),
            "directories without memory.md must be excluded"
        );

        let has_status = projects.iter().find(|p| p.name == "has-status").unwrap();
        assert_eq!(has_status.category, "ai");
        assert_eq!(has_status.status, "active");
        assert!(has_status.github.is_none());
        assert!(has_status.last_commit.is_none());

        let no_status = projects.iter().find(|p| p.name == "no-status").unwrap();
        assert_eq!(
            no_status.status, "—",
            "projects without a parseable status line still get returned, not filtered"
        );
    }

    #[test]
    fn has_sigmap_context_true_for_legacy_sigmap_md() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("SIGMAP.md"), "# SigMap\n").unwrap();
        assert!(has_sigmap_context(tmp.path()));
    }

    #[test]
    fn has_sigmap_context_true_for_current_sigmap_init_marker() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("gen-context.config.json"), "{}").unwrap();
        assert!(has_sigmap_context(tmp.path()));
    }

    #[test]
    fn has_sigmap_context_false_when_neither_marker_present() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("AGENTS.md"), "# hand-written, not sigmap\n").unwrap();
        assert!(!has_sigmap_context(tmp.path()));
    }
}
