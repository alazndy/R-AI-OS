use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct MemProject {
    pub name: String,
    pub path: PathBuf,
    pub status: String,
    pub date: String,
    pub has_memory: bool,
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
    // Projects are immediate subdirectories of the room folder.
    // Deeper nesting (like Sistem Araçları/R-AI-OS) is handled by checking
    // both direct children and one level deeper.
    let mut projects: Vec<(MemProject, SystemTime)> = Vec::new();

    let walker = WalkDir::new(room_path)
        .max_depth(4)
        .min_depth(0)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !n.starts_with('.') && n != "node_modules" && n != "target"
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir());

    for entry in walker {
        let proj_path = entry.path().to_path_buf();
        // Improved memory detection: check common variations and locations
        let memory_path = find_memory_file(&proj_path);
        let has_memory = memory_path.is_some();

        let is_project = has_memory
            || proj_path.join("Cargo.toml").exists()
            || proj_path.join("package.json").exists()
            || proj_path.join("go.mod").exists()
            || proj_path.join("requirements.txt").exists()
            || proj_path.join("pyproject.toml").exists()
            || proj_path.join("platformio.ini").exists()
            || proj_path.join(".git").exists()
            || proj_path.join(".agents").exists();

        if !is_project {
            continue;
        }

        let name = entry
            .file_name()
            .to_string_lossy()
            .into_owned();

        let (status, date, mtime) = if let Some(ref mp) = memory_path {
            read_memory_status(mp)
        } else {
            ("(no memory.md)".into(), "—".into(), SystemTime::UNIX_EPOCH)
        };

        projects.push((
            MemProject { name, path: proj_path, status, date, has_memory },
            mtime,
        ));
    }

    projects.sort_by(|a, b| b.1.cmp(&a.1));
    projects.into_iter().map(|(p, _)| p).collect()
}

/// Extract the first status/date line from memory.md.
/// Looks for "Tarih:" in "Son Durum" section, then falls back to
/// the first bullet under any section.
fn read_memory_status(path: &Path) -> (String, String, SystemTime) {
    let mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return ("(unreadable)".into(), "—".into(), mtime),
    };

    let date = extract_date(&content);
    let status = extract_status(&content);

    (status, date, mtime)
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

