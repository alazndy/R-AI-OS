use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{anyhow, Result};

pub fn sync_universe(dev_ops: &Path, master: &Path) -> Result<String> {
    if !master.exists() {
        return Err(anyhow!("MASTER.md not found at {}", master.display()));
    }

    let mut count = 0u32;

    for cat_entry in fs::read_dir(dev_ops)?.filter_map(|e| e.ok()) {
        if !cat_entry.path().is_dir() {
            continue;
        }
        let cat_name = cat_entry.file_name().to_string_lossy().into_owned();
        if cat_name.starts_with('.') || cat_name == "_eski" || cat_name == "AI OS" {
            continue;
        }

        for proj_entry in fs::read_dir(cat_entry.path())?.filter_map(|e| e.ok()) {
            if !proj_entry.path().is_dir() {
                continue;
            }
            let proj_name = proj_entry.file_name().to_string_lossy().into_owned();
            if proj_name.starts_with('.') {
                continue;
            }

            let proj_path = proj_entry.path();

            link_file(master, &proj_path.join("CLAUDE.md"));
            link_file(master, &proj_path.join("GEMINI.md"));
            ensure_memory(&proj_path.join("memory.md"), &proj_name);

            count += 1;
        }
    }

    Ok(format!("Universe Synchronized: {} projects aligned with MASTER rules.", count))
}

fn link_file(src: &Path, dst: &PathBuf) {
    if dst.exists() {
        let _ = fs::remove_file(dst);
    }
    let _ = fs::hard_link(src, dst);
}

fn ensure_memory(path: &PathBuf, project_name: &str) {
    if path.exists() {
        return;
    }
    let content = format!(
        "# {} Memory\n\n## Son Durum\n- Tarih: {}\n\n## Claude\n### Yaptıkları\n- Initialized via AI OS\n",
        project_name,
        chrono_now()
    );
    let _ = fs::write(path, content);
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let y = 1970 + days / 365;
    format!("{}-01-01", y)
}
