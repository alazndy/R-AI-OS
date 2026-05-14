use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheDir {
    pub path: PathBuf,
    pub kind: String,
    pub bytes: u64,
}

impl CacheDir {
    pub fn mb(&self) -> f64 {
        self.bytes as f64 / 1_048_576.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskReport {
    pub path: PathBuf,
    pub total_bytes: u64,
    pub source_bytes: u64,
    pub cache_bytes: u64,
    pub cache_dirs: Vec<CacheDir>,
    pub largest_files: Vec<(PathBuf, u64)>,
    pub file_count: usize,
}

impl DiskReport {
    pub fn total_mb(&self) -> f64 {
        self.total_bytes as f64 / 1_048_576.0
    }
    pub fn source_mb(&self) -> f64 {
        self.source_bytes as f64 / 1_048_576.0
    }
    pub fn cache_mb(&self) -> f64 {
        self.cache_bytes as f64 / 1_048_576.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanResult {
    pub cleaned_dirs: Vec<PathBuf>,
    pub freed_bytes: u64,
    pub errors: Vec<String>,
}

impl CleanResult {
    pub fn freed_mb(&self) -> f64 {
        self.freed_bytes as f64 / 1_048_576.0
    }
}

// ─── Cache dir patterns ───────────────────────────────────────────────────────

const CACHE_DIRS: &[(&str, &str)] = &[
    ("target", "Rust build"),
    ("node_modules", "Node deps"),
    (".next", "Next.js cache"),
    ("dist", "Build output"),
    ("build", "Build output"),
    (".cache", "Tool cache"),
    ("__pycache__", "Python cache"),
    (".pytest_cache", "Pytest cache"),
    (".mypy_cache", "Mypy cache"),
    (".ruff_cache", "Ruff cache"),
    (".gradle", "Gradle cache"),
    (".kotlin", "Kotlin cache"),
    ("vendor", "Go/PHP vendor"),
];

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn analyze(dir: &Path) -> DiskReport {
    let mut cache_dirs: Vec<CacheDir> = Vec::new();
    let mut largest: Vec<(PathBuf, u64)> = Vec::new();
    let mut file_count = 0usize;
    let mut source_bytes = 0u64;

    let total_bytes = dir_size(dir);

    // Find cache dirs (top-level only to avoid double-counting)
    for (name, kind) in CACHE_DIRS {
        let candidate = dir.join(name);
        if candidate.exists() && candidate.is_dir() {
            let bytes = dir_size(&candidate);
            cache_dirs.push(CacheDir {
                path: candidate,
                kind: kind.to_string(),
                bytes,
            });
        }
    }

    let cache_bytes: u64 = cache_dirs.iter().map(|c| c.bytes).sum();

    // Walk source files for LOC / largest
    walk_source(dir, &mut largest, &mut file_count, &mut source_bytes);
    largest.sort_by(|a, b| b.1.cmp(&a.1));
    largest.truncate(10);

    DiskReport {
        path: dir.to_path_buf(),
        total_bytes,
        source_bytes,
        cache_bytes,
        cache_dirs,
        largest_files: largest,
        file_count,
    }
}

/// Analyze all known projects and return sorted by total size.
pub fn analyze_all(dev_ops: &Path) -> Vec<DiskReport> {
    if let Ok(conn) = crate::db::open_db() {
        if let Ok(projects) = crate::db::load_all_projects(&conn) {
            let mut reports: Vec<DiskReport> = projects
                .iter()
                .map(|p| std::path::Path::new(&p.path))
                .filter(|p| p.exists())
                .map(analyze)
                .collect();
            reports.sort_by(|a, b| b.total_bytes.cmp(&a.total_bytes));
            return reports;
        }
    }
    // Fallback: scan dev_ops root
    vec![analyze(dev_ops)]
}

/// Remove all detected cache directories. Returns what was freed.
pub fn clean(dir: &Path, dry_run: bool) -> CleanResult {
    let report = analyze(dir);
    let mut result = CleanResult {
        cleaned_dirs: vec![],
        freed_bytes: 0,
        errors: vec![],
    };

    for cache in &report.cache_dirs {
        if dry_run {
            result.cleaned_dirs.push(cache.path.clone());
            result.freed_bytes += cache.bytes;
        } else {
            match std::fs::remove_dir_all(&cache.path) {
                Ok(_) => {
                    result.cleaned_dirs.push(cache.path.clone());
                    result.freed_bytes += cache.bytes;
                }
                Err(e) => result
                    .errors
                    .push(format!("{}: {}", cache.path.display(), e)),
            }
        }
    }

    result
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                dir_size(&p)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

const SOURCE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "kt", "swift", "go", "java", "cpp", "c", "h", "md",
    "toml", "yaml", "json",
];

fn walk_source(dir: &Path, largest: &mut Vec<(PathBuf, u64)>, count: &mut usize, bytes: &mut u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }
        if CACHE_DIRS.iter().any(|(n, _)| *n == name_str.as_ref()) {
            continue;
        }

        if path.is_dir() {
            walk_source(&path, largest, count, bytes);
        } else {
            let is_source = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| SOURCE_EXTS.contains(&e))
                .unwrap_or(false);

            if is_source {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                *count += 1;
                *bytes += size;
                largest.push((path, size));
            }
        }
    }
}

pub fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1_073_741_824 => format!("{:.1} GB", b as f64 / 1_073_741_824.0),
        b if b >= 1_048_576 => format!("{:.1} MB", b as f64 / 1_048_576.0),
        b if b >= 1_024 => format!("{:.0} KB", b as f64 / 1_024.0),
        b => format!("{} B", b),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(id: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("raios_disk_{}", id));
        let _ = std::fs::create_dir_all(&p);
        p
    }

    #[test]
    fn human_size_formats() {
        assert_eq!(human_size(500), "500 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(2 * 1_048_576), "2.0 MB");
        assert_eq!(human_size(3 * 1_073_741_824), "3.0 GB");
    }

    #[test]
    fn analyze_empty_dir() {
        let dir = tmp("empty");
        let r = analyze(&dir);
        assert_eq!(r.total_bytes, 0);
        assert_eq!(r.cache_dirs.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_cache_dir() {
        let dir = tmp("cache");
        let target = dir.join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("big_artifact"), vec![0u8; 1024]).unwrap();

        let r = analyze(&dir);
        assert_eq!(r.cache_dirs.len(), 1);
        assert_eq!(r.cache_dirs[0].kind, "Rust build");
        assert!(r.cache_bytes > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_dry_run_does_not_delete() {
        let dir = tmp("clean_dry");
        let nm = dir.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("pkg.js"), b"code").unwrap();

        let result = clean(&dir, true);
        assert!(!result.cleaned_dirs.is_empty());
        assert!(nm.exists(), "dry run should not delete");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_removes_cache() {
        let dir = tmp("clean_real");
        let nm = dir.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("pkg.js"), b"code").unwrap();

        let result = clean(&dir, false);
        assert!(!result.cleaned_dirs.is_empty());
        assert!(!nm.exists(), "should be deleted");
        assert!(result.freed_bytes > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn analyze_raios_project() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let r = analyze(&root);
        assert!(r.file_count > 0);
        assert!(r.source_bytes > 0);
    }
}
