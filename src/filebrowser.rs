use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub read_only: bool,
    pub exists: bool,
}

impl FileEntry {
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let exists = path.exists();
        Self {
            name: name.into(),
            path,
            read_only: false,
            exists,
        }
    }

    pub fn readonly(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub fn exists(&self) -> bool {
        self.exists
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

// ── Agent rule groups ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AgentRuleGroup {
    pub agent: String,
    pub icon: &'static str,
    pub config_dir: String,
    pub files: Vec<FileEntry>,
}

impl AgentRuleGroup {
    fn new(agent: impl Into<String>, icon: &'static str, config_dir: impl Into<String>) -> Self {
        Self {
            agent: agent.into(),
            icon,
            config_dir: config_dir.into(),
            files: Vec::new(),
        }
    }

    pub fn exists(&self) -> bool {
        !self.files.is_empty()
    }
}

/// Dynamically discovers rule/config files for all known AI agents.
pub fn discover_all_agent_rules(dev_ops: &Path) -> Vec<AgentRuleGroup> {
    let h = home();
    let mut groups = Vec::new();

    // ── Claude Code ───────────────────────────────────────────────────────────
    {
        let mut g = AgentRuleGroup::new("Claude Code", "◆", "~/.claude/");
        let claude_dir = h.join(".claude");

        // Global CLAUDE.md (home root)
        let global_claude = h.join("CLAUDE.md");
        if global_claude.exists() {
            g.files
                .push(FileEntry::new("CLAUDE.md (Global)", global_claude));
        }
        // ~/.claude/rules/*.md
        let rules_dir = claude_dir.join("rules");
        if let Ok(entries) = fs::read_dir(&rules_dir) {
            let mut rule_files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
                .collect();
            rule_files.sort();
            for p in rule_files {
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                g.files.push(FileEntry::new(name, p));
            }
        }
        // settings.json
        let settings = claude_dir.join("settings.json");
        if settings.exists() {
            g.files
                .push(FileEntry::new("settings.json", settings).readonly());
        }
        // settings.local.json
        let local = claude_dir.join("settings.local.json");
        if local.exists() {
            g.files.push(FileEntry::new("settings.local.json", local));
        }
        // CLAUDE.md inside .claude/
        let inner_claude = claude_dir.join("CLAUDE.md");
        if inner_claude.exists() {
            g.files
                .push(FileEntry::new("CLAUDE.md (.claude/)", inner_claude));
        }
        groups.push(g);
    }

    // ── Antigravity ───────────────────────────────────────────────────────────
    {
        let mut g = AgentRuleGroup::new("Antigravity", "⬡", "~/.agents/");
        let agents_dir = h.join(".agents");

        // hooks dir manifest
        let hooks_dir = agents_dir.join("hooks");
        if hooks_dir.is_dir() {
            g.files
                .push(FileEntry::new("hooks/ (dir)", hooks_dir).readonly());
        }
        // .md files directly under .agents/
        if let Ok(entries) = fs::read_dir(&agents_dir) {
            let mut files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && matches!(
                            p.extension().and_then(|x| x.to_str()),
                            Some("md") | Some("json") | Some("toml")
                        )
                })
                .collect();
            files.sort();
            for p in files {
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                g.files.push(FileEntry::new(name, p));
            }
        }
        groups.push(g);
    }

    // ── OpenCode ────────────────────────────────────────────────────────────
    {
        let mut g = AgentRuleGroup::new("OpenCode", "◈", "~/.config/opencode/");
        let oc_dir = h.join(".config").join("opencode");

        // opencode.jsonc
        let config_json = oc_dir.join("opencode.jsonc");
        if config_json.exists() {
            g.files.push(FileEntry::new("opencode.jsonc", config_json));
        }
        let config_json = oc_dir.join("opencode.json");
        if config_json.exists() {
            g.files.push(FileEntry::new("opencode.json", config_json));
        }
        // skills dir
        let skills_dir = oc_dir.join("skills");
        if skills_dir.is_dir() {
            g.files
                .push(FileEntry::new("skills/ (dir)", skills_dir).readonly());
        }
        // plugins dir
        let plugins_dir = oc_dir.join("plugins");
        if plugins_dir.is_dir() {
            g.files
                .push(FileEntry::new("plugins/ (dir)", plugins_dir).readonly());
        }
        // AGENTS.md symlink in home
        let agents_link = h.join("AGENTS.md");
        if agents_link.exists() {
            g.files
                .push(FileEntry::new("AGENTS.md (home symlink)", agents_link));
        }
        groups.push(g);
    }

    // ── Cursor ────────────────────────────────────────────────────────────────
    // Only show if Cursor is actually installed (config dir or binary present)
    {
        let cursor_dir = h.join(".cursor");
        let cursor_binary = crate::core::process::resolve_command_path("cursor").is_some();
        if cursor_dir.exists() || cursor_binary {
            let mut g = AgentRuleGroup::new("Cursor", "⊕", "~/.cursor/");

            let rules_dir = cursor_dir.join("rules");
            if let Ok(entries) = fs::read_dir(&rules_dir) {
                let mut files: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .collect();
                files.sort();
                for p in files {
                    let name = format!(
                        "rules/{}",
                        p.file_name().unwrap_or_default().to_string_lossy()
                    );
                    g.files.push(FileEntry::new(name, p));
                }
            }
            let mcp = cursor_dir.join("mcp.json");
            if mcp.exists() {
                g.files.push(FileEntry::new("mcp.json", mcp).readonly());
            }
            collect_project_rules(dev_ops, ".cursorrules", "cursorrules", &mut g.files, 3);
            groups.push(g);
        }
    }

    // ── Windsurf ──────────────────────────────────────────────────────────────
    {
        let mut g = AgentRuleGroup::new("Windsurf", "≋", "~/.windsurf/");
        let ws_dir = h.join(".windsurf");

        if let Ok(entries) = fs::read_dir(&ws_dir) {
            let mut files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect();
            files.sort();
            for p in files {
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                g.files.push(FileEntry::new(name, p));
            }
        }
        // .windsurfrules in dev_ops
        collect_project_rules(dev_ops, ".windsurfrules", "windsurfrules", &mut g.files, 3);

        groups.push(g);
    }

    // ── GitHub Copilot ────────────────────────────────────────────────────────
    {
        let mut g = AgentRuleGroup::new("GitHub Copilot", "○", ".github/");

        // .github/copilot-instructions.md in dev_ops projects
        collect_project_rules(
            dev_ops,
            "copilot-instructions.md",
            "copilot-instructions",
            &mut g.files,
            5,
        );

        groups.push(g);
    }

    // ── Jules ─────────────────────────────────────────────────────────────────
    {
        let mut g = AgentRuleGroup::new("Jules (Google)", "✦", ".github/");

        // AGENTS.md or JULES.md files in dev_ops projects
        collect_project_rules(dev_ops, "AGENTS.md", "AGENTS.md", &mut g.files, 3);
        collect_project_rules(dev_ops, "JULES.md", "JULES.md", &mut g.files, 3);

        groups.push(g);
    }

    groups
}

/// Walk dev_ops looking for files with exact name `target_name`, up to `limit`.
fn collect_project_rules(
    dev_ops: &Path,
    target_name: &str,
    display_prefix: &str,
    out: &mut Vec<FileEntry>,
    limit: usize,
) {
    if out.len() >= limit {
        return;
    }
    let walker = WalkDir::new(dev_ops)
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !matches!(
                n.as_ref(),
                "node_modules" | "target" | ".git" | "dist" | ".next"
            )
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());

    for entry in walker {
        if out.len() >= limit {
            break;
        }
        if entry.file_name().to_string_lossy() == target_name {
            let project = entry
                .path()
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "root".to_string());
            let name = format!("{} [{}]", display_prefix, project);
            out.push(FileEntry::new(name, entry.path().to_path_buf()));
        }
    }
}

// ── Static file lists (home-relative, no config needed) ──────────────────────

pub fn get_master_rule_files(master_md: &Path) -> Vec<FileEntry> {
    let h = home();
    vec![
        FileEntry::new("MASTER.md (Constitution)", master_md.to_path_buf()).readonly(),
        FileEntry::new("CLAUDE.md (Global)", h.join("CLAUDE.md")),
        FileEntry::new(
            "hardware-rules.md",
            h.join(".claude/rules/hardware-rules.md"),
        ),
        FileEntry::new("ui-rules.md", h.join(".claude/rules/ui-rules.md")),
        FileEntry::new("web-rules.md", h.join(".claude/rules/web-rules.md")),
    ]
}

pub fn get_agent_config_files() -> Vec<FileEntry> {
    let h = home();
    vec![
        FileEntry::new("Claude settings.json", h.join(".claude/settings.json")),
        FileEntry::new("Claude hooks", h.join(".agents/hooks")),
    ]
}

/// Policy files — includes MASTER.md whose path comes from config.
pub fn get_policy_files() -> Vec<FileEntry> {
    let h = home();
    vec![
        FileEntry::new("Claude settings.json", h.join(".claude/settings.json")).readonly(),
    ]
}

/// MemPalace files — dev_ops_path comes from config.
pub fn get_mempalace_files(dev_ops: &Path) -> Vec<FileEntry> {
    let mut entries = vec![
        FileEntry::new("mempalace.yaml", dev_ops.join("mempalace.yaml")),
        FileEntry::new("entities.json", dev_ops.join("entities.json")),
    ];
    entries.extend(discover_memory_files(dev_ops, 6));
    entries
}

pub fn discover_memory_files(base: &Path, limit: usize) -> Vec<FileEntry> {
    let skip_dirs: &[&str] = &[
        "node_modules", "target", "dist", "build", ".next", "__pycache__",
        "vendor", ".turbo", "out", "coverage", "cache", "tmp", "temp",
        "logs", "log", "runs", "test", "tests", "__tests__", "e2e",
        "fixtures", "examples", "samples", "gradle", ".gradle",
        "cmake-build-debug", "cmake-build-release", ".idea", ".vscode",
    ];

    let walker = WalkDir::new(base)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            if name.starts_with('.') {
                return false;
            }
            !skip_dirs.contains(&name.as_ref())
        });

    // Collect all memory.md paths with modification times
    let mut found: Vec<(PathBuf, SystemTime)> = Vec::new();
    for entry in walker.filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy().to_lowercase() == "memory.md" {
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            found.push((entry.path().to_path_buf(), modified));
        }
    }

    // Sort shallowest first to establish parent-before-child order
    found.sort_by_key(|a| a.0.components().count());

    // Remove nested memory.md files: if a parent project already accepted,
    // do not include sub-project memory.md files under that parent
    let mut accepted_dirs: Vec<PathBuf> = Vec::new();
    let mut deduped: Vec<(PathBuf, SystemTime)> = Vec::new();
    for (path, mtime) in found {
        // The "project dir" is the directory containing memory.md
        let proj_dir = match path.parent() {
            Some(d) => d.to_path_buf(),
            None => continue,
        };
        let is_nested = accepted_dirs.iter().any(|a| proj_dir.starts_with(a));
        if !is_nested {
            accepted_dirs.push(proj_dir);
            deduped.push((path, mtime));
        }
    }

    // Re-sort by modification time (most recent first) for display
    deduped.sort_by_key(|a| std::cmp::Reverse(a.1));

    deduped
        .into_iter()
        .take(limit)
        .map(|(path, _)| {
            let proj = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            FileEntry::new(format!("{}/memory.md", proj), path)
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct RecentProject {
    pub name: String,
    pub rel_path: String,
    pub changes: Vec<String>,
    pub git_dirty: Option<bool>,
    pub git_branch: Option<String>,
}

pub fn load_recent_projects(base: &Path) -> Vec<RecentProject> {
    let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();

    let walker = WalkDir::new(base)
        .max_depth(5)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.')
                && name != "node_modules"
                && name != "target"
                && name != "dist"
                && name != ".next"
        });

    for entry in walker.filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy() == "memory.md" {
            let t = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((entry.path().to_path_buf(), t));
        }
    }

    files.sort_by_key(|a| std::cmp::Reverse(a.1));

    files
        .into_iter()
        .take(3)
        .map(|(path, _)| {
            let name = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rel = path
                .strip_prefix(base)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let changes = extract_changes(&path);
            let project_dir = path.parent().unwrap_or(&path).to_path_buf();
            let git_dirty = git_is_dirty(&project_dir);
            let git_branch = git_current_branch(&project_dir);
            RecentProject {
                name,
                rel_path: rel,
                changes,
                git_dirty,
                git_branch,
            }
        })
        .collect()
}

fn extract_changes(path: &PathBuf) -> Vec<String> {
    let Ok(content) = fs::read_to_string(path) else {
        return vec![];
    };
    let mut changes = Vec::new();
    let mut collecting = false;

    for line in content.lines() {
        if line.contains("Yaptıkları") || line.contains("Claude") {
            collecting = true;
            continue;
        }
        if collecting {
            let trimmed = line.trim();
            if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                changes.push(trimmed[2..].to_string());
                if changes.len() >= 3 {
                    break;
                }
            } else if (line.starts_with("##") || line.starts_with("# "))
                && !line.contains("Claude")
                && !changes.is_empty()
            {
                break;
            }
        }
    }
    changes
}

pub fn load_file_content(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| format!("# Error\n\nCould not read:\n  {}\n\n{}", path.display(), e))
}

pub fn save_file_content(path: &Path, content: &str) -> std::io::Result<()> {
    crate::safe_io::safe_write(path, content).map_err(std::io::Error::other)
}

pub fn find_file_by_name(query: &str, master_md: &Path) -> Option<FileEntry> {
    let q = query.to_lowercase();
    let lists = [
        get_master_rule_files(master_md),
        get_agent_config_files(),
        get_policy_files(),
    ];
    for list in &lists {
        for entry in list {
            if entry.name.to_lowercase().contains(&q)
                || entry.path.to_string_lossy().to_lowercase().contains(&q)
            {
                return Some(entry.clone());
            }
        }
    }
    let p = PathBuf::from(query);
    if p.exists() {
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        return Some(FileEntry::new(name, p));
    }
    None
}

// ── Git helpers ───────────────────────────────────────────────────────────────

pub fn git_get_remote_url(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if url.is_empty() {
            None
        } else {
            Some(url)
        }
    } else {
        None
    }
}

pub fn git_is_dirty(dir: &Path) -> Option<bool> {
    let out = Command::new("git")
        .args(["status", "--short"])
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        Some(!out.stdout.is_empty())
    } else {
        None
    }
}

fn git_current_branch(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if branch.is_empty() {
            None
        } else {
            Some(branch)
        }
    } else {
        None
    }
}

pub fn get_git_log(dir: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["log", "--oneline", "-20", "--no-color"])
        .current_dir(dir)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::to_owned)
            .collect(),
        _ => vec!["(not a git repo or no history)".into()],
    }
}

#[derive(Debug)]
pub struct GitCommitResult {
    pub committed: bool,
    pub pushed: bool,
    pub message: String,
}

pub fn git_commit(dir: &Path, msg: &str) -> GitCommitResult {
    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir)
        .output();

    if add.map(|o| !o.status.success()).unwrap_or(true) {
        return GitCommitResult {
            committed: false,
            pushed: false,
            message: "git add failed".into(),
        };
    }

    let commit = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(dir)
        .output();

    match commit {
        Ok(o) if o.status.success() => GitCommitResult {
            committed: true,
            pushed: false,
            message: "ok".into(),
        },
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            let nothing_to_commit =
                stderr.contains("nothing to commit") || stderr.contains("nothing added");
            if nothing_to_commit {
                GitCommitResult {
                    committed: false,
                    pushed: false,
                    message: "nothing to commit".into(),
                }
            } else {
                GitCommitResult {
                    committed: false,
                    pushed: false,
                    message: stderr,
                }
            }
        }
        Err(e) => GitCommitResult {
            committed: false,
            pushed: false,
            message: e.to_string(),
        },
    }
}

pub fn git_push(dir: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .args(["push", "origin", "HEAD"])
        .current_dir(dir)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
