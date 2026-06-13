use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

// ─── Core Validation ─────────────────────────────────────────────────────────

/// Validates if `target` path is strictly within the `workspace` directory.
/// Resolves paths to absolute/canonical forms to prevent traversal (e.g. `..`).
pub fn validate_path(target: &Path, workspace: &Path) -> Result<PathBuf> {
    let canonical_workspace = workspace
        .canonicalize()
        .map_err(|e| anyhow!("Workspace path {:?} not accessible: {}", workspace, e))?;

    // Target might not exist yet (e.g. when writing a new file),
    // so we canonicalize the parent and append the file name.
    let canonical_target = if target.exists() {
        target
            .canonicalize()
            .map_err(|e| anyhow!("Target path {:?} canonicalization failed: {}", target, e))?
    } else {
        let parent = target
            .parent()
            .ok_or_else(|| anyhow!("Target path has no parent: {:?}", target))?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|e| anyhow!("Parent path {:?} not accessible: {}", parent, e))?;
        let file_name = target
            .file_name()
            .ok_or_else(|| anyhow!("Target path has no filename: {:?}", target))?;
        canonical_parent.join(file_name)
    };

    if canonical_target.starts_with(&canonical_workspace) {
        Ok(canonical_target)
    } else {
        Err(anyhow!(
            "Access Denied: {:?} is outside the allowed workspace {:?}",
            target,
            workspace
        ))
    }
}

/// Returns `true` if the path is safe to access within the given workspace.
pub fn is_path_safe(target: &Path, workspace: &Path) -> bool {
    validate_path(target, workspace).is_ok()
}

// ─── SandboxGuard ─────────────────────────────────────────────────────────────

/// Configurable sandbox guard. Holds the allowed workspace root
/// plus any explicit extra blocked paths (e.g. ~/.ssh, C:/Windows).
pub struct SandboxGuard {
    pub workspace: PathBuf,
    pub blocked_paths: Vec<PathBuf>,
}

impl SandboxGuard {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            blocked_paths: vec![],
        }
    }

    pub fn with_blocked_paths(mut self, paths: Vec<String>) -> Self {
        self.blocked_paths = paths.into_iter().map(PathBuf::from).collect();
        self
    }

    /// Validates a target path: must be inside workspace AND not in a blocked path.
    pub fn check(&self, target: &Path) -> Result<PathBuf> {
        // Explicit block list check (before canonicalization so it works even if path
        // doesn't exist yet).
        let target_str = target.to_string_lossy().to_lowercase();
        for blocked in &self.blocked_paths {
            let blocked_str = blocked.to_string_lossy().to_lowercase();
            if target_str.starts_with(&blocked_str as &str) {
                return Err(anyhow!(
                    "Access Denied: {:?} matches a blocked path {:?}",
                    target,
                    blocked
                ));
            }
        }
        // Workspace boundary check
        validate_path(target, &self.workspace)
    }

    pub fn is_allowed(&self, target: &Path) -> bool {
        self.check(target).is_ok()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().to_path_buf();
        (tmp, ws)
    }

    #[test]
    fn allows_existing_file_inside_workspace() {
        let (_tmp, ws) = setup();
        let target = ws.join("src/lib.rs");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "").unwrap();
        assert!(is_path_safe(&target, &ws));
    }

    #[test]
    fn allows_new_file_inside_workspace() {
        let (_tmp, ws) = setup();
        // The parent dir exists but the file doesn't yet
        let target = ws.join("new_file.rs");
        assert!(is_path_safe(&target, &ws));
    }

    #[test]
    fn blocks_traversal_outside_workspace() {
        let (_tmp, ws) = setup();
        let target = ws.join("../outside.rs");
        assert!(!is_path_safe(&target, &ws));
    }

    #[test]
    fn blocks_absolute_path_outside_workspace() {
        let (_tmp, ws) = setup();
        let target = PathBuf::from("C:/Windows/System32/evil.dll");
        assert!(!is_path_safe(&target, &ws));
    }

    #[test]
    fn sandbox_guard_blocks_explicit_blocked_path() {
        let (_tmp, ws) = setup();
        // .ssh is "outside" the workspace but we also use blocked_paths
        let guard = SandboxGuard::new(ws.clone())
            .with_blocked_paths(vec!["C:/Users/turha/.ssh".to_string()]);
        assert!(!guard.is_allowed(Path::new("C:/Users/turha/.ssh/id_rsa")));
    }

    #[test]
    fn sandbox_guard_allows_inside_workspace() {
        let (_tmp, ws) = setup();
        let target = ws.join("src/main.rs");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "").unwrap();
        let guard = SandboxGuard::new(ws);
        assert!(guard.is_allowed(&target));
    }
}
