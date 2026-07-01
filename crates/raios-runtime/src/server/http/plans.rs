use serde_json::json;
use std::path::{Path, PathBuf};

pub(super) fn locate_plans_dir() -> Option<PathBuf> {
    let suffix = Path::new("docs").join("superpowers").join("plans");

    if let Ok(exe) = std::env::current_exe() {
        if let Some(target) = exe.parent().and_then(|p| p.parent()) {
            let candidate = target.parent().unwrap_or(target).join(&suffix);
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join(&suffix);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    None
}

pub(super) fn scan_plans(dir: &Path) -> Vec<serde_json::Value> {
    let mut entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return vec![],
    };
    entries.sort_by_key(|e| e.file_name());

    entries
        .iter()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                return None;
            }
            let slug = path.file_stem()?.to_string_lossy().to_string();
            let content = std::fs::read_to_string(&path).unwrap_or_default();

            let title = content
                .lines()
                .find(|l| l.starts_with("# "))
                .map(|l| l.trim_start_matches("# ").to_string())
                .unwrap_or_else(|| slug.clone());

            let checked = content.matches("- [x]").count() + content.matches("- [X]").count();
            let unchecked = content.matches("- [ ]").count();
            let total = checked + unchecked;
            let pct: u8 = checked
                .checked_mul(100)
                .and_then(|v| v.checked_div(total))
                .map(|v| v.min(100) as u8)
                .unwrap_or(0);

            let status = match (checked, unchecked) {
                (0, 0) => "no_tasks",
                (0, _) => "not_started",
                (_, 0) => "done",
                _ => "in_progress",
            };

            let date = slug.get(..10).unwrap_or("").to_string();

            Some(json!({
                "slug": slug,
                "title": title,
                "date": date,
                "status": status,
                "checked": checked,
                "total": total,
                "pct": pct,
            }))
        })
        .collect()
}
