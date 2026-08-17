use std::path::Path;
use std::process::Command;

use raios_core::entities::EntityProject;

struct ProjectSnapshot {
    name: String,
    dirty_files: usize,
    last_commit_days: Option<u64>,
    has_readme: bool,
    has_memory: bool,
    has_sigmap: bool,
    memory_stale_days: Option<u64>,
}

pub fn cmd_reflect(dev_ops_path: &Path, json: bool) {
    let projects = raios_core::entities::discover_entities(dev_ops_path);
    if projects.is_empty() {
        eprintln!("No projects found in {}", dev_ops_path.display());
        return;
    }

    let snapshots: Vec<ProjectSnapshot> = projects.iter().map(snapshot).collect();

    if json {
        print_json(&snapshots);
    } else {
        print_report(&snapshots);
    }
}

fn snapshot(p: &EntityProject) -> ProjectSnapshot {
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

fn print_report(snaps: &[ProjectSnapshot]) {
    let total = snaps.len();
    let dirty_count = snaps.iter().filter(|s| s.dirty_files > 0).count();
    let stale_count = snaps
        .iter()
        .filter(|s| s.last_commit_days.is_some_and(|d| d > 14))
        .count();

    let readme_ok = snaps.iter().filter(|s| s.has_readme).count();
    let memory_ok = snaps.iter().filter(|s| s.has_memory).count();
    let sigmap_ok = snaps.iter().filter(|s| s.has_sigmap).count();

    let score = calculate_score(snaps);

    println!();
    println!("━━━ WORKSPACE REFLECTION ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "  Projects: {}  │  dirty: {}  │  stale (>14d): {}",
        total, dirty_count, stale_count
    );
    println!();

    // Attention required
    let attention: Vec<_> = snaps
        .iter()
        .filter(|s| {
            s.dirty_files > 0
                || !s.has_readme
                || !s.has_memory
                || !s.has_sigmap
                || s.memory_stale_days.is_some_and(|d| d > 7)
                || s.last_commit_days.is_some_and(|d| d > 14)
        })
        .collect();

    if attention.is_empty() {
        println!("  ✓ All projects look healthy.");
    } else {
        println!("── ATTENTION REQUIRED ──────────────────────────────────────");
        for s in &attention {
            let mut flags = Vec::new();
            if s.dirty_files > 0 {
                flags.push(format!("dirty:{}", s.dirty_files));
            }
            if let Some(d) = s.last_commit_days {
                if d > 14 {
                    flags.push(format!("stale:{}d", d));
                }
            }
            if !s.has_readme {
                flags.push("no README.md".into());
            }
            if !s.has_memory {
                flags.push("no memory.md".into());
            }
            if !s.has_sigmap {
                flags.push("no SIGMAP.md".into());
            }
            if let Some(d) = s.memory_stale_days {
                if d > 7 {
                    flags.push(format!("memory stale:{}d", d));
                }
            }
            println!("  ● {:<24} {}", s.name, flags.join("  "));
        }
    }

    println!();
    println!("── DOCS COVERAGE ───────────────────────────────────────────");
    println!(
        "  README.md  {}/{} present{}",
        readme_ok,
        total,
        if readme_ok < total {
            format!("  ✗ {} missing", total - readme_ok)
        } else {
            "  ✓".into()
        }
    );
    println!(
        "  memory.md  {}/{} present{}",
        memory_ok,
        total,
        if memory_ok < total {
            format!("  ✗ {} missing", total - memory_ok)
        } else {
            "  ✓".into()
        }
    );
    println!(
        "  SIGMAP.md  {}/{} present{}",
        sigmap_ok,
        total,
        if sigmap_ok < total {
            format!("  ✗ {} missing", total - sigmap_ok)
        } else {
            "  ✓".into()
        }
    );

    if stale_count > 0 {
        println!();
        println!("── STALE PROJECTS (no commit > 14d) ────────────────────────");
        for s in snaps
            .iter()
            .filter(|s| s.last_commit_days.is_some_and(|d| d > 14))
        {
            println!(
                "  ● {:<24} last commit: {}d ago",
                s.name,
                s.last_commit_days.unwrap_or(0)
            );
        }
    }

    println!();
    let bar_filled = (score / 10) as usize;
    let bar: String = "█".repeat(bar_filled) + &"░".repeat(10 - bar_filled);
    println!("── OVERALL SCORE ───────────────────────────────────────────");
    println!("  {}  {}/100", bar, score);

    // Recommendations
    let recs = build_recommendations(snaps);
    if !recs.is_empty() {
        println!();
        println!("── RECOMMENDATIONS ─────────────────────────────────────────");
        for (i, rec) in recs.iter().enumerate() {
            println!("  {}. {}", i + 1, rec);
        }
    }
    println!();
}

fn calculate_score(snaps: &[ProjectSnapshot]) -> u8 {
    if snaps.is_empty() {
        return 100;
    }
    let total = snaps.len() as f32;
    let dirty_penalty = snaps.iter().filter(|s| s.dirty_files > 0).count() as f32 * 3.0;
    let readme_penalty = snaps.iter().filter(|s| !s.has_readme).count() as f32 * 2.0;
    let memory_penalty = snaps.iter().filter(|s| !s.has_memory).count() as f32 * 2.0;
    let sigmap_penalty = snaps.iter().filter(|s| !s.has_sigmap).count() as f32 * 1.0;
    let stale_penalty = snaps
        .iter()
        .filter(|s| s.last_commit_days.is_some_and(|d| d > 14))
        .count() as f32
        * 2.0;
    let mem_stale_penalty = snaps
        .iter()
        .filter(|s| s.memory_stale_days.is_some_and(|d| d > 7))
        .count() as f32
        * 1.0;

    let total_penalty = dirty_penalty
        + readme_penalty
        + memory_penalty
        + sigmap_penalty
        + stale_penalty
        + mem_stale_penalty;

    let raw = 100.0 - (total_penalty / total * 10.0);
    raw.clamp(0.0, 100.0) as u8
}

fn build_recommendations(snaps: &[ProjectSnapshot]) -> Vec<String> {
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

fn print_json(snaps: &[ProjectSnapshot]) {
    let items: Vec<serde_json::Value> = snaps
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "dirty_files": s.dirty_files,
                "last_commit_days": s.last_commit_days,
                "has_readme": s.has_readme,
                "has_memory": s.has_memory,
                "has_sigmap": s.has_sigmap,
                "memory_stale_days": s.memory_stale_days,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "score": calculate_score(snaps),
            "projects": items,
        }))
        .unwrap_or_default()
    );
}

#[cfg(test)]
mod tests {
    use super::{build_recommendations, calculate_score, ProjectSnapshot};

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
    fn calculate_score_of_empty_snapshot_list_is_perfect() {
        assert_eq!(calculate_score(&[]), 100);
    }

    #[test]
    fn calculate_score_of_all_healthy_projects_is_perfect() {
        let snaps = vec![healthy("a"), healthy("b"), healthy("c")];
        assert_eq!(calculate_score(&snaps), 100);
    }

    #[test]
    fn calculate_score_deducts_points_for_each_penalty_category() {
        // total_penalty = 3 (dirty) + 2 (no readme) + 2 (no memory) + 1 (no sigmap)
        //               + 2 (stale > 14d) + 1 (stale memory > 7d) = 11
        // raw = 100 - (11 / 1 * 10) = -10 -> clamps to 0
        let snaps = vec![ProjectSnapshot {
            name: "broken".into(),
            dirty_files: 5,
            last_commit_days: Some(30),
            has_readme: false,
            has_memory: false,
            has_sigmap: false,
            memory_stale_days: Some(20),
        }];
        assert_eq!(calculate_score(&snaps), 0);
    }

    #[test]
    fn calculate_score_never_goes_below_zero() {
        fn broken(name: &str) -> ProjectSnapshot {
            ProjectSnapshot {
                name: name.into(),
                dirty_files: 99,
                last_commit_days: Some(999),
                has_readme: false,
                has_memory: false,
                has_sigmap: false,
                memory_stale_days: Some(999),
            }
        }
        let snaps = vec![broken("a"), broken("b")];
        assert_eq!(calculate_score(&snaps), 0);
    }

    #[test]
    fn calculate_score_weighs_dirty_files_by_presence_not_by_count() {
        // Only the *presence* of dirty files matters, not how many — a project
        // with 1 dirty file and one with 500 both contribute the same 3.0 penalty.
        let one_dirty = ProjectSnapshot {
            dirty_files: 1,
            ..healthy("one-dirty")
        };
        let many_dirty = ProjectSnapshot {
            dirty_files: 500,
            ..healthy("many-dirty")
        };
        assert_eq!(
            calculate_score(&[one_dirty]),
            calculate_score(&[many_dirty])
        );
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
