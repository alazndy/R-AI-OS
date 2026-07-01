use std::path::Path;
use std::process::Command;

/// Best-effort `git diff --stat HEAD` in the target project.
/// Returns `None` on any failure or when there is no diff.
pub fn diff_stat(project_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["diff", "--stat", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stat = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stat.is_empty() {
        None
    } else {
        Some(stat)
    }
}
