//! Context-budget measurement for `raios run`.
//!
//! A project whose source is too large to hand to an agent raw gets a compact
//! signature map (`SIGMAP.md`, produced by the `sigmap` tool) instead. This
//! module answers the two questions that decision needs:
//!
//! * [`measure`] — does the project's *source* exceed the budget? Build output
//!   (`target/`), dependency trees (`node_modules/`), VCS data (`.git/`) and
//!   other hidden directories are not source and are never counted. Symlinks
//!   are never followed, so a link cycle cannot loop forever. The walk stops
//!   the moment the budget is exceeded, so the cost is bounded by the budget,
//!   not by the size of the tree.
//! * [`sigmap_is_fresh`] — is `SIGMAP.md` already newer than every source
//!   file, so regenerating it would be wasted work?

use std::fs::Metadata;
use std::ops::ControlFlow;
use std::path::Path;
use std::time::SystemTime;

/// Directory names that never hold project source.
const SKIPPED_DIRS: &[&str] = &["target", "node_modules", "__pycache__", "dist"];

/// Name of the signature map the `sigmap` tool writes at the project root.
const SIGMAP_FILE: &str = "SIGMAP.md";

/// Result of [`measure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    /// Bytes counted. When `exceeded` is true the walk stopped early, so this
    /// is a lower bound ("at least"), not the total.
    pub bytes: u64,
    pub exceeded: bool,
}

/// Visits every non-skipped regular file under `root`. The visitor can stop
/// the walk early by returning `ControlFlow::Break`.
fn walk_sources(root: &Path, mut visit: impl FnMut(&Path, &Metadata) -> ControlFlow<()>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // `DirEntry::file_type` does not traverse symlinks: links are
            // neither descended into nor counted.
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !(name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_ref())) {
                    stack.push(entry.path());
                }
            } else if file_type.is_file() {
                let Ok(meta) = entry.metadata() else { continue };
                if visit(&entry.path(), &meta).is_break() {
                    return;
                }
            }
        }
    }
}

/// Measures the source size of the project at `root` against `limit_bytes`.
/// A path that is not a directory measures as zero.
pub fn measure(root: &Path, limit_bytes: u64) -> Measured {
    let mut bytes = 0u64;
    let mut exceeded = false;
    walk_sources(root, |_, meta| {
        bytes += meta.len();
        if bytes > limit_bytes {
            exceeded = true;
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    Measured { bytes, exceeded }
}

/// True if `SIGMAP.md` exists and no source file is newer than it.
///
/// Markdown files do not affect a code signature map, and the agent wrapper
/// appends to `memory.md` every few seconds during a session, so `.md` files
/// are ignored here (including `SIGMAP.md` itself).
pub fn sigmap_is_fresh(root: &Path) -> bool {
    let Ok(map_modified) = std::fs::metadata(root.join(SIGMAP_FILE)).and_then(|m| m.modified())
    else {
        return false;
    };
    let mut fresh = true;
    walk_sources(root, |path, meta| {
        let is_markdown = path.extension().is_some_and(|ext| ext == "md");
        let newer = meta
            .modified()
            .map(|modified: SystemTime| modified > map_modified)
            .unwrap_or(false);
        if !is_markdown && newer {
            fresh = false;
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    fresh
}

#[cfg(test)]
mod tests;
