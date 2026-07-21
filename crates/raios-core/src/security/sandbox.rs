use anyhow::{anyhow, Result};
use std::path::{Component, Path, PathBuf};

// ─── Core Validation ─────────────────────────────────────────────────────────

/// Resolves `target` to an absolute form suitable for security comparisons.
/// If `target` exists, resolves it via the real filesystem (following
/// symlinks). If it doesn't exist yet (e.g. a file about to be written),
/// canonicalizes the parent directory and re-appends the file name — this
/// still resolves any `..`/symlink components that appear in the parent
/// chain, which is what makes `..` traversal ineffective against the checks
/// below.
fn resolve_target(target: &Path) -> Result<PathBuf> {
    if target.exists() {
        target
            .canonicalize()
            .map_err(|e| anyhow!("Target path {:?} canonicalization failed: {}", target, e))
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
        Ok(canonical_parent.join(file_name))
    }
}

/// Best-effort normalization for paths that may not exist on disk — e.g.
/// blocklist entries like `~/.ssh` or `C:/Windows` configured for a platform
/// other than the one currently running. Falls back to purely lexical
/// resolution of `.`/`..` components (no filesystem access) so blocklist
/// comparisons still have a stable, traversal-resistant form to compare
/// against even when `canonicalize()` can't be used.
fn normalize_best_effort(path: &Path) -> PathBuf {
    if let Ok(canon) = path.canonicalize() {
        return canon;
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

/// Component-based `starts_with`, case-insensitive on Windows (where the
/// filesystem itself is case-insensitive) and case-sensitive everywhere
/// else. Always compares whole path components, never raw substrings, so a
/// blocked path like `/ws/.ssh` can't be defeated by a sibling such as
/// `/ws/.sshfoo` nor bypassed by a `..` segment left in the string form.
fn path_starts_with(target: &Path, prefix: &Path) -> bool {
    #[cfg(windows)]
    {
        let mut t = target.components();
        for p_comp in prefix.components() {
            match t.next() {
                Some(t_comp) => {
                    let a = t_comp.as_os_str().to_string_lossy().to_lowercase();
                    let b = p_comp.as_os_str().to_string_lossy().to_lowercase();
                    if a != b {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    #[cfg(not(windows))]
    {
        target.starts_with(prefix)
    }
}

/// Validates if `target` path is strictly within the `workspace` directory.
/// Resolves paths to absolute/canonical forms to prevent traversal (e.g. `..`).
pub fn validate_path(target: &Path, workspace: &Path) -> Result<PathBuf> {
    let canonical_workspace = workspace
        .canonicalize()
        .map_err(|e| anyhow!("Workspace path {:?} not accessible: {}", workspace, e))?;
    let canonical_target = resolve_target(target)?;

    if path_starts_with(&canonical_target, &canonical_workspace) {
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
    ///
    /// Both checks run against the resolved (canonicalized-or-lexically-
    /// normalized) form of `target` — never the raw, unresolved string — so a
    /// `..` segment can't be used to slip past either the blocklist or the
    /// workspace boundary. Blocklist is checked first (defense in depth: an
    /// explicitly blocked path stays blocked even if it happens to sit inside
    /// the workspace), then the workspace boundary itself.
    pub fn check(&self, target: &Path) -> Result<PathBuf> {
        let canonical_target = resolve_target(target)?;

        for blocked in &self.blocked_paths {
            let canonical_blocked = normalize_best_effort(blocked);
            if path_starts_with(&canonical_target, &canonical_blocked) {
                return Err(anyhow!(
                    "Access Denied: {:?} matches a blocked path {:?}",
                    target,
                    blocked
                ));
            }
        }

        let canonical_workspace = self
            .workspace
            .canonicalize()
            .map_err(|e| anyhow!("Workspace path {:?} not accessible: {}", self.workspace, e))?;

        if path_starts_with(&canonical_target, &canonical_workspace) {
            Ok(canonical_target)
        } else {
            Err(anyhow!(
                "Access Denied: {:?} is outside the allowed workspace {:?}",
                target,
                self.workspace
            ))
        }
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

    /// Regression test: the old blocklist check string-matched the raw,
    /// unresolved target path, so `sub/../.secrets/key` (which lexically and
    /// canonically resolves *into* the blocked `.secrets` dir) never matched
    /// the `starts_with` check on the raw string and slipped through.
    #[test]
    fn sandbox_guard_blocklist_not_bypassable_via_dotdot() {
        let (_tmp, ws) = setup();
        let sub = ws.join("sub");
        let secret = ws.join(".secrets");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(&secret).unwrap();

        let guard = SandboxGuard::new(ws.clone())
            .with_blocked_paths(vec![secret.to_string_lossy().to_string()]);

        let traversal_target = sub.join("..").join(".secrets").join("key");
        assert!(!guard.is_allowed(&traversal_target));
    }

    /// Regression test: a blocked path must not falsely match a sibling that
    /// merely shares a string prefix (e.g. `.ssh` vs `.sshfoo`) now that the
    /// comparison is component-based instead of a raw string `starts_with`.
    #[test]
    fn sandbox_guard_blocklist_does_not_over_match_sibling_prefix() {
        let (_tmp, ws) = setup();
        let blocked_dir = ws.join(".ssh");
        let sibling_dir = ws.join(".sshfoo");
        std::fs::create_dir_all(&blocked_dir).unwrap();
        std::fs::create_dir_all(&sibling_dir).unwrap();

        let guard = SandboxGuard::new(ws.clone())
            .with_blocked_paths(vec![blocked_dir.to_string_lossy().to_string()]);

        assert!(guard.is_allowed(&sibling_dir));
        assert!(!guard.is_allowed(&blocked_dir.join("id_rsa")));
    }
}
