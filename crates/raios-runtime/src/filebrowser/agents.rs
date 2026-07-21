use super::FileEntry;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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

pub fn discover_all_agent_rules(dev_ops: &Path) -> Vec<AgentRuleGroup> {
    let h = super::home();
    let mut groups = Vec::new();

    // ── Claude Code ───────────────────────────────────────────────────────────
    {
        let mut g = AgentRuleGroup::new("Claude Code", "◆", "~/.claude/");
        let claude_dir = h.join(".claude");

        let global_claude = h.join("CLAUDE.md");
        if global_claude.exists() {
            g.files
                .push(FileEntry::new("CLAUDE.md (Global)", global_claude));
        }
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
        let settings = claude_dir.join("settings.json");
        if settings.exists() {
            g.files
                .push(FileEntry::new("settings.json", settings).readonly());
        }
        let local = claude_dir.join("settings.local.json");
        if local.exists() {
            g.files.push(FileEntry::new("settings.local.json", local));
        }
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

        let hooks_dir = agents_dir.join("hooks");
        if hooks_dir.is_dir() {
            g.files
                .push(FileEntry::new("hooks/ (dir)", hooks_dir).readonly());
        }
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

    // ── OpenCode ──────────────────────────────────────────────────────────────
    {
        let mut g = AgentRuleGroup::new("OpenCode", "◈", "~/.config/opencode/");
        let oc_dir = h.join(".config").join("opencode");

        let config_json = oc_dir.join("opencode.jsonc");
        if config_json.exists() {
            g.files.push(FileEntry::new("opencode.jsonc", config_json));
        }
        let config_json = oc_dir.join("opencode.json");
        if config_json.exists() {
            g.files.push(FileEntry::new("opencode.json", config_json));
        }
        let skills_dir = oc_dir.join("skills");
        if skills_dir.is_dir() {
            g.files
                .push(FileEntry::new("skills/ (dir)", skills_dir).readonly());
        }
        let plugins_dir = oc_dir.join("plugins");
        if plugins_dir.is_dir() {
            g.files
                .push(FileEntry::new("plugins/ (dir)", plugins_dir).readonly());
        }
        let agents_link = h.join("AGENTS.md");
        if agents_link.exists() {
            g.files
                .push(FileEntry::new("AGENTS.md (home symlink)", agents_link));
        }
        groups.push(g);
    }

    // ── Cursor ────────────────────────────────────────────────────────────────
    {
        let cursor_dir = h.join(".cursor");
        let cursor_binary = raios_core::core::process::resolve_command_path("cursor").is_some();
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
        collect_project_rules(dev_ops, ".windsurfrules", "windsurfrules", &mut g.files, 3);

        groups.push(g);
    }

    // ── GitHub Copilot ────────────────────────────────────────────────────────
    {
        let mut g = AgentRuleGroup::new("GitHub Copilot", "○", ".github/");

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

        collect_project_rules(dev_ops, "AGENTS.md", "AGENTS.md", &mut g.files, 3);
        collect_project_rules(dev_ops, "JULES.md", "JULES.md", &mut g.files, 3);

        groups.push(g);
    }

    groups
}

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
