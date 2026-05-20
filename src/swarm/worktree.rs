use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

/// Create an isolated Git worktree for a swarm task.
/// Returns (worktree_path, branch_name).
pub fn create_worktree(
    project_path: &Path,
    task_id: Uuid,
    task_description: &str,
) -> Result<(PathBuf, String)> {
    let slug = make_slug(task_description);
    let branch = format!("swarm/{}-{}", &task_id.to_string()[..8], slug);

    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let worktree_base = home
        .join(".raios")
        .join("worktrees")
        .join(project_path.file_name().unwrap_or_default());
    let worktree_path = worktree_base.join(task_id.to_string());

    std::fs::create_dir_all(&worktree_base).context("Failed to create worktree base directory")?;

    let status = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            &branch,
            worktree_path.to_str().unwrap_or("."),
            "HEAD",
        ])
        .current_dir(project_path)
        .status()
        .context("Failed to run git worktree add")?;

    anyhow::ensure!(
        status.success(),
        "git worktree add failed with status: {}",
        status
    );

    Ok((worktree_path, branch))
}

/// Remove a worktree and prune stale entries.
pub fn remove_worktree(project_path: &Path, worktree_path: &Path) -> Result<()> {
    Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_path.to_str().unwrap_or("."),
        ])
        .current_dir(project_path)
        .status()
        .context("Failed to run git worktree remove")?;

    Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(project_path)
        .status()
        .context("Failed to run git worktree prune")?;

    Ok(())
}

/// List all active worktrees for a project.
pub fn list_worktrees(project_path: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_path)
        .output()
        .context("Failed to run git worktree list")?;

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let paths: Vec<String> = text
        .lines()
        .filter(|l| l.starts_with("worktree "))
        .map(|l| l["worktree ".len()..].to_string())
        .collect();

    Ok(paths)
}

fn make_slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_replaces_spaces() {
        assert_eq!(make_slug("feat: add dark mode"), "feat-add-dark-mode");
    }

    #[test]
    fn slug_truncates_long_descriptions() {
        let long = "implement full authentication system with oauth and jwt";
        let slug = make_slug(long);
        assert!(slug.split('-').count() <= 5);
    }
}
