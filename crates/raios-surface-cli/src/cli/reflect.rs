use std::path::Path;

use raios_runtime::reflect_scoring::{build_recommendations, snapshot, ProjectSnapshot};

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
    use super::{calculate_score, ProjectSnapshot};

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
}
