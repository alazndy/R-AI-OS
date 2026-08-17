use std::path::Path;
use std::process::Command;

use raios_core::entities::EntityProject;

pub struct ProjectSnapshot {
    pub name: String,
    pub dirty_files: usize,
    pub last_commit_days: Option<u64>,
    pub has_readme: bool,
    pub has_memory: bool,
    pub has_sigmap: bool,
    pub memory_stale_days: Option<u64>,
}

pub fn snapshot(p: &EntityProject) -> ProjectSnapshot {
    let dirty_files = count_dirty_files(&p.local_path);
    let last_commit_days = git_days_since_last_commit(&p.local_path);
    let has_readme = p.local_path.join("README.md").exists();
    let has_memory = p.local_path.join("memory.md").exists();
    let has_sigmap = raios_core::entities::has_sigmap_context(&p.local_path);
    let memory_stale_days = if has_memory {
        file_age_days(&p.local_path.join("memory.md"))
    } else {
        None
    };

    ProjectSnapshot {
        name: p.name.clone(),
        dirty_files,
        last_commit_days,
        has_readme,
        has_memory,
        has_sigmap,
        memory_stale_days,
    }
}

fn count_dirty_files(dir: &Path) -> usize {
    let out = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "status", "--porcelain"])
        .output()
        .ok();
    out.map(|o| {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .count()
    })
    .unwrap_or(0)
}

fn git_days_since_last_commit(dir: &Path) -> Option<u64> {
    let out = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "log", "-1", "--format=%ct"])
        .output()
        .ok()?;
    let ts: i64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some(((now - ts).max(0) / 86400) as u64)
}

fn file_age_days(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let age = std::time::SystemTime::now().duration_since(modified).ok()?;
    Some(age.as_secs() / 86400)
}

pub fn build_recommendations(snaps: &[ProjectSnapshot]) -> Vec<String> {
    let mut recs = Vec::new();

    let dirty: Vec<_> = snaps
        .iter()
        .filter(|s| s.dirty_files > 0)
        .map(|s| s.name.as_str())
        .collect();
    if !dirty.is_empty() {
        recs.push(format!(
            "Commit or stash dirty changes: {}",
            dirty.join(", ")
        ));
    }

    let no_memory: usize = snaps.iter().filter(|s| !s.has_memory).count();
    if no_memory > 0 {
        recs.push(format!(
            "Create memory.md in {} project(s) — use standard template",
            no_memory
        ));
    }

    let no_sigmap: usize = snaps.iter().filter(|s| !s.has_sigmap).count();
    if no_sigmap > 0 {
        recs.push(format!(
            "Run `sigmap` in {} project(s) to generate SIGMAP.md",
            no_sigmap
        ));
    }

    let stale_mem: Vec<_> = snaps
        .iter()
        .filter(|s| s.memory_stale_days.is_some_and(|d| d > 7))
        .map(|s| s.name.as_str())
        .collect();
    if !stale_mem.is_empty() {
        recs.push(format!(
            "Update memory.md (>7d stale): {}",
            stale_mem.join(", ")
        ));
    }

    recs
}

#[cfg(test)]
mod tests {
    use super::{build_recommendations, ProjectSnapshot};

    fn healthy(name: &str) -> ProjectSnapshot {
        ProjectSnapshot {
            name: name.into(),
            dirty_files: 0,
            last_commit_days: Some(1),
            has_readme: true,
            has_memory: true,
            has_sigmap: true,
            memory_stale_days: Some(1),
        }
    }

    #[test]
    fn build_recommendations_of_healthy_projects_is_empty() {
        let snaps = vec![healthy("a"), healthy("b")];
        assert!(build_recommendations(&snaps).is_empty());
    }

    #[test]
    fn build_recommendations_lists_dirty_projects_by_name() {
        let snaps = vec![
            ProjectSnapshot {
                dirty_files: 2,
                ..healthy("dirty-one")
            },
            healthy("clean-one"),
        ];
        let recs = build_recommendations(&snaps);
        assert_eq!(recs.len(), 1);
        assert!(recs[0].contains("dirty-one"));
        assert!(!recs[0].contains("clean-one"));
    }

    #[test]
    fn build_recommendations_counts_missing_memory_and_sigmap_files() {
        let snaps = vec![
            ProjectSnapshot {
                has_memory: false,
                ..healthy("no-memory")
            },
            ProjectSnapshot {
                has_sigmap: false,
                ..healthy("no-sigmap")
            },
        ];
        let recs = build_recommendations(&snaps);
        assert!(recs
            .iter()
            .any(|r| r.contains("memory.md") && r.contains('1')));
        assert!(recs
            .iter()
            .any(|r| r.contains("SIGMAP.md") && r.contains('1')));
    }

    #[test]
    fn build_recommendations_lists_stale_memory_projects_by_name() {
        let snaps = vec![ProjectSnapshot {
            memory_stale_days: Some(30),
            ..healthy("stale-memory")
        }];
        let recs = build_recommendations(&snaps);
        assert_eq!(recs.len(), 1);
        assert!(recs[0].contains("stale-memory"));
    }

    #[test]
    fn build_recommendations_orders_dirty_memory_sigmap_then_stale_memory() {
        let snaps = vec![ProjectSnapshot {
            dirty_files: 1,
            has_memory: false,
            has_sigmap: false,
            memory_stale_days: None,
            ..healthy("everything-wrong")
        }];
        let recs = build_recommendations(&snaps);
        assert_eq!(recs.len(), 3);
        assert!(recs[0].starts_with("Commit or stash"));
        assert!(recs[1].starts_with("Create memory.md"));
        assert!(recs[2].starts_with("Run `sigmap`"));
    }
}
