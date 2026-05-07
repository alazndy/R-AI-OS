use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct MemProject {
    pub name: String,
    pub path: PathBuf,
    pub status: String,
    pub date: String,
    pub has_memory: bool,
    pub version: Option<String>,
    pub version_nickname: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemRoom {
    pub folder_name: String,
    pub icon: &'static str,
    pub projects: Vec<MemProject>,
}

/// Walk Dev Ops and build the full MemPalace — all rooms, all projects.
/// Each top-level category folder = one room.
/// Each immediate subfolder of a room = one project.
pub fn build(dev_ops: &Path) -> Vec<MemRoom> {
    let mut rooms: Vec<MemRoom> = Vec::new();

    let mut entries: Vec<(String, PathBuf)> = std::fs::read_dir(dev_ops)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().ok().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(name, _)| !name.starts_with('.') && !name.starts_with('_'))
        .collect();

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (folder_name, folder_path) in entries {
        let icon = room_icon(&folder_name);
        let projects = scan_projects(&folder_path);
        if !projects.is_empty() {
            rooms.push(MemRoom { folder_name, icon, projects });
        }
    }

    rooms
}

fn scan_projects(room_path: &Path) -> Vec<MemProject> {
    // Two-pass scan: depth 1 then depth 2.
    // If a depth-1 dir IS a project root, add it and do NOT recurse into it.
    // If a depth-1 dir is NOT a project (i.e. it's a sub-category like "CLI_Tools"),
    // scan its immediate children as potential projects.
    // This prevents monorepos and project internals from being counted as separate projects.
    let mut projects: Vec<(MemProject, SystemTime)> = Vec::new();

    let Ok(level1_iter) = std::fs::read_dir(room_path) else { return vec![] };

    for entry1 in level1_iter.filter_map(|e| e.ok()) {
        let Ok(ft) = entry1.file_type() else { continue };
        if !ft.is_dir() { continue; }

        let name1 = entry1.file_name().to_string_lossy().into_owned();
        if name1.starts_with('.') || is_skip_dir(&name1) { continue; }

        let path1 = entry1.path();

        if is_project_root(&path1) {
            // Level-1 dir is itself a project — stop here, don't go deeper
            if let Some(proj) = make_project(path1) {
                projects.push(proj);
            }
        } else {
            // Level-1 dir is a sub-category (e.g. "CLI_Tools", "Projects") — scan its children
            let Ok(level2_iter) = std::fs::read_dir(&path1) else { continue };

            for entry2 in level2_iter.filter_map(|e| e.ok()) {
                let Ok(ft2) = entry2.file_type() else { continue };
                if !ft2.is_dir() { continue; }

                let name2 = entry2.file_name().to_string_lossy().into_owned();
                if name2.starts_with('.') || is_skip_dir(&name2) { continue; }

                let path2 = entry2.path();
                if is_project_root(&path2) {
                    if let Some(proj) = make_project(path2) {
                        projects.push(proj);
                    }
                }
            }
        }
    }

    projects.sort_by(|a, b| b.1.cmp(&a.1));
    projects.into_iter().map(|(p, _)| p).collect()
}

fn is_skip_dir(name: &str) -> bool {
    matches!(name,
        "node_modules" | "target" | "dist" | "build" | ".next"
        | "__pycache__" | "vendor" | ".turbo" | "out"
    )
}

fn is_project_root(path: &Path) -> bool {
    path.join(".git").exists()
        || path.join("memory.md").exists()
        || path.join("Cargo.toml").exists()
        || path.join("package.json").exists()
        || path.join("go.mod").exists()
        || path.join("pyproject.toml").exists()
        || path.join("platformio.ini").exists()
        || path.join(".agents").exists()
}

fn make_project(path: PathBuf) -> Option<(MemProject, SystemTime)> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let memory_path = find_memory_file(&path);
    let has_memory = memory_path.is_some();

    let (status, date, version, version_nickname, mtime) = if let Some(ref mp) = memory_path {
        read_memory_status(mp)
    } else {
        ("(no memory.md)".into(), "—".into(), None, None, SystemTime::UNIX_EPOCH)
    };

    Some((
        MemProject { name, path, status, date, has_memory, version, version_nickname },
        mtime,
    ))
}

/// Extract the first status/date line from memory.md.
/// Looks for "Tarih:" in "Son Durum" section, then falls back to
/// the first bullet under any section.
fn read_memory_status(path: &Path) -> (String, String, Option<String>, Option<String>, SystemTime) {
    let mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return ("(unreadable)".into(), "—".into(), None, None, mtime),
    };

    let date = extract_date(&content);
    let status = extract_status(&content);
    let version = extract_version(&content);
    let version_nickname = extract_version_nickname(&content);

    (status, date, version, version_nickname, mtime)
}

fn extract_version(content: &str) -> Option<String> {
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("- Sürüm:") || t.starts_with("Sürüm:") || t.starts_with("- Version:") || t.starts_with("Version:") {
            let val = t.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
            if !val.is_empty() && val != "—" {
                return Some(val);
            }
        }
    }
    None
}

fn extract_version_nickname(content: &str) -> Option<String> {
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("- Sürüm Adı:") || t.starts_with("Sürüm Adı:") || t.starts_with("- Nickname:") || t.starts_with("Nickname:") {
            let val = t.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
            if !val.is_empty() && val != "—" {
                return Some(val);
            }
        }
    }
    None
}

fn extract_date(content: &str) -> String {
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("- Tarih:") || t.starts_with("Tarih:") {
            let val = t
                .splitn(2, ':')
                .nth(1)
                .unwrap_or("")
                .trim()
                .to_string();
            if !val.is_empty() && val != "—" {
                return val;
            }
        }
    }
    "—".into()
}

fn extract_status(content: &str) -> String {
    let mut in_recent = false;
    for line in content.lines() {
        let t = line.trim();

        // Find "Son Durum", "Yaptıkları", "Aktif" sections
        if t.contains("Son Durum") || t.starts_with("## Son") {
            in_recent = true;
            continue;
        }
        // Stop at next section
        if in_recent && (t.starts_with("## ") || t.starts_with("# ")) {
            break;
        }
        if in_recent && (t.starts_with("- ") || t.starts_with("* ")) {
            let s = t[2..].trim().to_string();
            if !s.is_empty() && s != "—" {
                let truncated: String = s.chars().take(80).collect();
                return truncated;
            }
        }
    }

    // Fallback: first bullet anywhere
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("- ") && !t.contains("Tarih") && !t.contains("agent") {
            let s = t[2..].trim();
            if !s.is_empty() && s.len() > 3 {
                let truncated: String = s.chars().take(80).collect();
                return truncated;
            }
        }
    }

    "—".into()
}

fn find_memory_file(proj_path: &Path) -> Option<PathBuf> {
    let variations = ["memory.md", "Memory.md", "MEMORY.md", "memory.MD", ".agents/memory.md"];
    for v in variations {
        let p = proj_path.join(v);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn room_icon(folder: &str) -> &'static str {
    let f = folder.to_lowercase();
    if f.contains("ai") && f.contains("veri") { return "🔬"; }
    if f.contains("ai") && f.contains("os") { return "🤖"; }
    if f.contains("crucix") { return "⚡"; }
    if f.contains("endüstriyel") || f.contains("saha") { return "🏭"; }
    if f.contains("kişisel") || f.contains("üretkenlik") { return "📋"; }
    if f.contains("medya") || f.contains("ses") { return "🎵"; }
    if f.contains("mobil") || f.contains("oyun") { return "📱"; }
    if f.contains("tasarım") || f.contains("geliştirici") { return "🎨"; }
    if f.contains("ui") && f.contains("altyapı") { return "🧩"; }
    if f.contains("web") && f.contains("app") { return "💻"; }
    if f.contains("web") && f.contains("platform") { return "🚀"; }
    if f.contains("iletişim") || f.contains("sosyal") { return "💬"; }
    "📁"
}

