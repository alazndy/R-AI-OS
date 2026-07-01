use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub diff_text: String,
}

/// Get a summary of changes between HEAD and the swarm branch.
pub fn diff_summary(project_path: &Path, branch: &str) -> Result<DiffSummary> {
    // Stat summary
    let stat = Command::new("git")
        .args(["diff", "--stat", &format!("HEAD...{branch}")])
        .current_dir(project_path)
        .output()
        .context("git diff --stat failed")?;

    let stat_text = String::from_utf8_lossy(&stat.stdout).to_string();
    let (files, ins, del) = parse_stat(&stat_text);

    // Full diff
    let diff = Command::new("git")
        .args(["diff", &format!("HEAD...{branch}")])
        .current_dir(project_path)
        .output()
        .context("git diff failed")?;

    Ok(DiffSummary {
        files_changed: files,
        insertions: ins,
        deletions: del,
        diff_text: String::from_utf8_lossy(&diff.stdout).to_string(),
    })
}

/// Merge the swarm branch into HEAD using --no-ff.
pub fn merge_branch(project_path: &Path, branch: &str, message: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["merge", "--no-ff", branch, "-m", message])
        .current_dir(project_path)
        .status()
        .context("git merge failed")?;

    anyhow::ensure!(status.success(), "git merge exited with status: {}", status);
    Ok(())
}

/// Delete a local branch (force).
pub fn delete_branch(project_path: &Path, branch: &str) -> Result<()> {
    Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(project_path)
        .status()
        .context("git branch -D failed")?;
    Ok(())
}

/// Parse "N files changed, X insertions(+), Y deletions(-)" from git diff --stat output.
fn parse_stat(text: &str) -> (usize, usize, usize) {
    let last = text.lines().last().unwrap_or("");
    (
        extract_num(last, "file"),
        extract_num(last, "insertion"),
        extract_num(last, "deletion"),
    )
}

fn extract_num(s: &str, keyword: &str) -> usize {
    let words: Vec<&str> = s.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        if word.contains(keyword) && i > 0 {
            if let Ok(n) = words[i - 1].parse::<usize>() {
                return n;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stat_extracts_numbers() {
        let text = " 3 files changed, 45 insertions(+), 2 deletions(-)";
        let (f, i, d) = parse_stat(text);
        assert_eq!(f, 3);
        assert_eq!(i, 45);
        assert_eq!(d, 2);
    }

    #[test]
    fn parse_stat_zero_deletions() {
        let text = " 1 file changed, 10 insertions(+)";
        let (f, i, d) = parse_stat(text);
        assert_eq!(f, 1);
        assert_eq!(i, 10);
        assert_eq!(d, 0);
    }
}
