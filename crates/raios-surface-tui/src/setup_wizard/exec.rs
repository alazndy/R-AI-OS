use std::path::Path;
use super::types::WizardAction;
use super::templates::{
    claude_md_template, codex_md_template, master_template, HOOKS_README,
    SKILL_CONTINUOUS_LEARNING, SKILL_GRAPHIFY, SKILL_KI_SNAPSHOT, SKILL_PROMPT_MASTER,
    SKILL_SEARCH_FIRST, SKILL_VERIFY,
};

/// Create the full Dev_Ops workspace directory structure.
pub fn exec_workspace(dev_ops: &Path, github_user: &str) -> Vec<WizardAction> {
    let mut log = Vec::new();

    let dirs = [
        "00_System/Agents",
        "00_System/Automation",
        "01_Hardware_&_Embedded/Control_Systems",
        "01_Hardware_&_Embedded/Firmware_&_Sensors",
        "02_AI_&_Data/Bots_&_Trading",
        "02_AI_&_Data/OS_&_Tools",
        "03_Core_Libraries/UI_Frameworks",
        "03_Core_Libraries/Utilities",
        "04_Web_Platforms/Management_Panels",
        "04_Web_Platforms/Public_Sites",
        "05_Mobile_&_Gaming/Apps",
        "05_Mobile_&_Gaming/Games",
        "06_Media_&_Audio/Audio_Projects",
        "06_Media_&_Audio/Visual_3D",
        "07_DevTools_&_Productivity/CLI_Tools",
        "07_DevTools_&_Productivity/Extensions",
        "07_DevTools_&_Productivity/Workflows",
        "08_External/Open_Source",
        "09_Archive/Legacy",
        "10_ADC/Projects",
        "10_ADC/Web_Sitesi",
        "11_Personal",
    ];

    for d in &dirs {
        let p = dev_ops.join(d);
        if p.exists() {
            log.push(WizardAction::skip(format!("exists: {}", d)));
        } else {
            match std::fs::create_dir_all(&p) {
                Ok(_) => log.push(WizardAction::ok(format!("created: {}", d))),
                Err(e) => log.push(WizardAction::fail(format!("failed {}: {}", d, e))),
            }
        }
    }

    let entities_path = dev_ops.join("entities.json");
    if !entities_path.exists() {
        let content = serde_json::json!({ "projects": [] });
        match std::fs::write(
            &entities_path,
            serde_json::to_string_pretty(&content).unwrap_or_default(),
        ) {
            Ok(_) => log.push(WizardAction::ok("created: entities.json")),
            Err(e) => log.push(WizardAction::fail(format!("entities.json: {}", e))),
        }
    } else {
        log.push(WizardAction::skip("exists: entities.json"));
    }

    let tasks_path = dev_ops.join("tasks.md");
    if !tasks_path.exists() {
        let content =
            "# Tasks\n\n<!-- raios task format: - [ ] Task text @agent #ProjectName -->\n";
        match std::fs::write(&tasks_path, content) {
            Ok(_) => log.push(WizardAction::ok("created: tasks.md")),
            Err(e) => log.push(WizardAction::fail(format!("tasks.md: {}", e))),
        }
    } else {
        log.push(WizardAction::skip("exists: tasks.md"));
    }

    let mp_path = dev_ops.join("mempalace.yaml");
    if !mp_path.exists() {
        let content = format!(
            "# MemPalace — Workspace Map\ngithub_user: \"{}\"\nrooms:\n  ai_os: \"00_System\"\n  hardware: \"01_Hardware_&_Embedded\"\n  ai_data: \"02_AI_&_Data\"\n  core_libs: \"03_Core_Libraries\"\n  web: \"04_Web_Platforms\"\n  mobile: \"05_Mobile_&_Gaming\"\n  media: \"06_Media_&_Audio\"\n  devtools: \"07_DevTools_&_Productivity\"\n  external: \"08_External\"\n  archive: \"09_Archive\"\n  adc: \"10_ADC\"\n  personal: \"11_Personal\"\n",
            github_user
        );
        match std::fs::write(&mp_path, content) {
            Ok(_) => log.push(WizardAction::ok("created: mempalace.yaml")),
            Err(e) => log.push(WizardAction::fail(format!("mempalace.yaml: {}", e))),
        }
    } else {
        log.push(WizardAction::skip("exists: mempalace.yaml"));
    }

    log
}

/// Create AGENT_CONSTITUTION.md and set up workspace symlinks.
pub fn exec_master(master_path: &Path, github_user: &str) -> Vec<WizardAction> {
    let mut log = Vec::new();

    if let Some(parent) = master_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if master_path.exists() {
        log.push(WizardAction::skip("exists: AGENT_CONSTITUTION.md"));
    } else {
        let content = master_template(github_user);
        match std::fs::write(master_path, content) {
            Ok(_) => log.push(WizardAction::ok(format!(
                "created: {}",
                master_path.display()
            ))),
            Err(e) => log.push(WizardAction::fail(format!("AGENT_CONSTITUTION.md: {}", e))),
        }
    }

    let home = dirs::home_dir().unwrap_or_default();
    for link_name in &["CLAUDE.md", "AGENTS.md"] {
        let link_path = home.join(link_name);
        if link_path.exists() || link_path.is_symlink() {
            log.push(WizardAction::skip(format!("exists: ~/{}", link_name)));
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            match symlink(master_path, &link_path) {
                Ok(_) => log.push(WizardAction::ok(format!(
                    "symlinked: ~/{} → AGENT_CONSTITUTION.md",
                    link_name
                ))),
                Err(e) => log.push(WizardAction::fail(format!("{}: {}", link_name, e))),
            }
        }
        #[cfg(windows)]
        {
            let content = format!("@{}\n", master_path.display());
            match std::fs::write(&link_path, content) {
                Ok(_) => log.push(WizardAction::ok(format!(
                    "created: ~/{} (references AGENT_CONSTITUTION.md)",
                    link_name
                ))),
                Err(e) => log.push(WizardAction::fail(format!("{}: {}", link_name, e))),
            }
        }
    }

    log
}

/// Set up Claude Code: CLAUDE.md template + MCP registration.
pub fn exec_claude(dev_ops: &Path, master_path: &Path) -> Vec<WizardAction> {
    let mut log = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();
    let claude_dir = home.join(".claude");
    let _ = std::fs::create_dir_all(&claude_dir);

    let claude_md = claude_dir.join("CLAUDE.md");
    if claude_md.exists() {
        log.push(WizardAction::skip("exists: ~/.claude/CLAUDE.md"));
    } else {
        let content = claude_md_template(master_path);
        match std::fs::write(&claude_md, content) {
            Ok(_) => log.push(WizardAction::ok("created: ~/.claude/CLAUDE.md")),
            Err(e) => log.push(WizardAction::fail(format!("CLAUDE.md: {}", e))),
        }
    }

    let settings_path = claude_dir.join("settings.json");
    match register_mcp_claude(&settings_path) {
        Ok(true) => log.push(WizardAction::ok(
            "registered: raios MCP in ~/.claude/settings.json",
        )),
        Ok(false) => log.push(WizardAction::skip("already registered: raios MCP")),
        Err(e) => log.push(WizardAction::fail(format!("MCP register: {}", e))),
    }

    let rules_dir = claude_dir.join("rules");
    let _ = std::fs::create_dir_all(&rules_dir);
    log.push(WizardAction::ok("created: ~/.claude/rules/"));

    let skills_dir = dev_ops.join(".agents").join("skills");
    let _ = std::fs::create_dir_all(&skills_dir);
    log.push(WizardAction::ok("created: .agents/skills/"));

    log
}

/// Set up Codex Kaira: ~/.codex/AGENTS.md
pub fn exec_codex(master_path: &Path) -> Vec<WizardAction> {
    let mut log = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();
    let codex_dir = home.join(".codex");
    let _ = std::fs::create_dir_all(&codex_dir);

    let agents_md = codex_dir.join("AGENTS.md");
    if agents_md.exists() {
        log.push(WizardAction::skip("exists: ~/.codex/AGENTS.md"));
    } else {
        let content = codex_md_template(master_path);
        match std::fs::write(&agents_md, content) {
            Ok(_) => log.push(WizardAction::ok("created: ~/.codex/AGENTS.md (Codex Kaira)")),
            Err(e) => log.push(WizardAction::fail(format!("AGENTS.md: {}", e))),
        }
    }

    log
}

/// Set up OpenCode: register raios MCP server.
pub fn exec_opencode() -> Vec<WizardAction> {
    let mut log = Vec::new();

    let existing = std::process::Command::new("opencode")
        .args(["mcp", "list"])
        .output();
    let already_registered = existing
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.contains("raios"))
        .unwrap_or(false);

    if already_registered {
        log.push(WizardAction::skip("already registered: raios MCP in opencode"));
    } else {
        let status = std::process::Command::new("opencode")
            .args(["mcp", "add", "raios", "--command", "raios", "--args", "mcp-server"])
            .status();
        match status {
            Ok(s) if s.success() => log.push(WizardAction::ok("registered: raios MCP in opencode")),
            Ok(_) => log.push(WizardAction::fail(
                "opencode mcp add failed — is opencode installed?",
            )),
            Err(e) => log.push(WizardAction::fail(format!("opencode mcp add: {}", e))),
        }
    }

    log
}

/// Create .agents/skills/ and .agents/hooks/ with starter files.
pub fn exec_skills(dev_ops: &Path) -> Vec<WizardAction> {
    let mut log = Vec::new();
    let agents_dir = dev_ops.join(".agents");

    let dirs_to_create = [agents_dir.join("skills"), agents_dir.join("hooks")];

    for d in &dirs_to_create {
        if d.exists() {
            log.push(WizardAction::skip(format!("exists: {}", d.display())));
        } else {
            match std::fs::create_dir_all(d) {
                Ok(_) => log.push(WizardAction::ok(format!("created: {}", d.display()))),
                Err(e) => log.push(WizardAction::fail(format!("{}: {}", d.display(), e))),
            }
        }
    }

    let skills_dir = agents_dir.join("skills");
    let skill_stubs = [
        ("prompt-master.md", SKILL_PROMPT_MASTER),
        ("graphify.md", SKILL_GRAPHIFY),
        ("verify-ai-os.md", SKILL_VERIFY),
        ("ki-snapshot.md", SKILL_KI_SNAPSHOT),
        ("search-first.md", SKILL_SEARCH_FIRST),
        ("continuous-learning.md", SKILL_CONTINUOUS_LEARNING),
    ];

    for (name, content) in &skill_stubs {
        let p = skills_dir.join(name);
        if p.exists() {
            log.push(WizardAction::skip(format!("exists: skills/{}", name)));
        } else {
            match std::fs::write(&p, content) {
                Ok(_) => log.push(WizardAction::ok(format!("created: skills/{}", name))),
                Err(e) => log.push(WizardAction::fail(format!("skills/{}: {}", name, e))),
            }
        }
    }

    let hooks_readme = agents_dir.join("hooks").join("README.md");
    if !hooks_readme.exists() {
        let _ = std::fs::write(&hooks_readme, HOOKS_README);
        log.push(WizardAction::ok("created: hooks/README.md"));
    }

    log
}

/// Install agent wrapper shell functions (or skip if choice != 0).
pub fn exec_agent_wrapper(choice: usize) -> Vec<WizardAction> {
    if choice != 0 {
        return vec![WizardAction::skip("agent wrapper: skipped")];
    }
    raios_runtime::agent_wrapper::install(raios_runtime::agent_wrapper::ALL_AGENTS)
        .into_iter()
        .map(|r| WizardAction { desc: r.desc, ok: r.ok, skipped: r.skipped })
        .collect()
}

/// Write config.toml and run initial discover.
pub fn exec_initialize(
    dev_ops: &Path,
    master_path: &Path,
    skills_path: &Path,
    vault_path: Option<&Path>,
    agent_wrapper_enabled: bool,
) -> Vec<WizardAction> {
    let mut log = Vec::new();

    let config = raios_core::config::Config {
        dev_ops_path: dev_ops.to_path_buf(),
        master_md_path: master_path.to_path_buf(),
        skills_path: skills_path.to_path_buf(),
        vault_projects_path: vault_path.map(|p| p.to_path_buf()).unwrap_or_default(),
        system_name: "k-ai-ra".to_string(),
        github_user: String::new(),
        agent_wrapper_enabled,
        daemon: Default::default(),
    };

    match config.save() {
        Ok(_) => log.push(WizardAction::ok("saved: ~/.config/raios/config.toml")),
        Err(e) => log.push(WizardAction::fail(format!("config.toml: {}", e))),
    }

    let projects = raios_core::entities::discover_entities(dev_ops);
    let count = projects.len();
    match raios_core::entities::save_entities(dev_ops, projects) {
        Ok(_) => log.push(WizardAction::ok(format!(
            "discovered {} projects → entities.json",
            count
        ))),
        Err(e) => log.push(WizardAction::fail(format!("discover: {}", e))),
    }

    log
}

fn register_mcp_claude(settings_path: &Path) -> Result<bool, String> {
    let mut json: serde_json::Value = if settings_path.exists() {
        let s = std::fs::read_to_string(settings_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&s).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if json.pointer("/mcpServers/raios").is_some() {
        return Ok(false);
    }

    let servers = json
        .as_object_mut()
        .ok_or("not an object")?
        .entry("mcpServers")
        .or_insert(serde_json::json!({}));

    servers["raios"] = serde_json::json!({
        "command": "raios",
        "args": ["mcp-server"]
    });

    let out = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(settings_path, out).map_err(|e| e.to_string())?;
    Ok(true)
}
