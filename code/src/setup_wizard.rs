use std::path::{Path, PathBuf};
use std::process::Command;

// ─── Steps ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Default)]
pub enum WizardStep {
    #[default]
    Welcome,
    Workspace,
    Master,
    Claude,
    Gemini,
    Antigravity,
    Skills,
    Initialize,
    Done,
}

impl WizardStep {
    pub fn next(&self) -> Self {
        match self {
            Self::Welcome     => Self::Workspace,
            Self::Workspace   => Self::Master,
            Self::Master      => Self::Claude,
            Self::Claude      => Self::Gemini,
            Self::Gemini      => Self::Antigravity,
            Self::Antigravity => Self::Skills,
            Self::Skills      => Self::Initialize,
            Self::Initialize  => Self::Done,
            Self::Done        => Self::Done,
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Self::Welcome     => 0,
            Self::Workspace   => 1,
            Self::Master      => 2,
            Self::Claude      => 3,
            Self::Gemini      => 4,
            Self::Antigravity => 5,
            Self::Skills      => 6,
            Self::Initialize  => 7,
            Self::Done        => 8,
        }
    }

    pub fn total() -> usize { 8 }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Welcome     => "WELCOME",
            Self::Workspace   => "WORKSPACE",
            Self::Master      => "MASTER.md",
            Self::Claude      => "CLAUDE CODE",
            Self::Gemini      => "GEMINI CLI",
            Self::Antigravity => "ANTIGRAVITY",
            Self::Skills      => "SKILLS & HOOKS",
            Self::Initialize  => "INITIALIZE",
            Self::Done        => "DONE",
        }
    }
}

// ─── Agent status ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    pub claude_installed: bool,
    pub claude_version: String,
    pub gemini_installed: bool,
    pub gemini_version: String,
    pub antigravity_installed: bool,
    pub antigravity_version: String,
    pub git_installed: bool,
    pub git_version: String,
    pub gh_installed: bool,
    pub gh_version: String,
}

pub fn detect_agents() -> AgentStatus {
    let mut s = AgentStatus::default();

    if let Some((ok, v)) = run_version(&["claude", "--version"]) {
        s.claude_installed = ok;
        s.claude_version = v;
    }
    if let Some((ok, v)) = run_version(&["gemini", "--version"]) {
        s.gemini_installed = ok;
        s.gemini_version = v;
    }
    if let Some((ok, v)) = run_version(&["antigravity", "--version"]) {
        s.antigravity_installed = ok;
        s.antigravity_version = v;
    }
    if let Some((ok, v)) = run_version(&["git", "--version"]) {
        s.git_installed = ok;
        s.git_version = v;
    }
    if let Some((ok, v)) = run_version(&["gh", "--version"]) {
        s.gh_installed = ok;
        s.gh_version = v.lines().next().unwrap_or("").to_string();
    }
    s
}

fn run_version(args: &[&str]) -> Option<(bool, String)> {
    let out = Command::new(args[0])
        .args(&args[1..])
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let v = if v.is_empty() {
        String::from_utf8_lossy(&out.stderr).trim().to_string()
    } else { v };
    Some((out.status.success(), v.lines().next().unwrap_or("").to_string()))
}

// ─── Execution ────────────────────────────────────────────────────────────────

pub struct WizardAction {
    pub desc: String,
    pub ok: bool,
    pub skipped: bool,
}

impl WizardAction {
    fn ok(desc: impl Into<String>) -> Self {
        Self { desc: desc.into(), ok: true, skipped: false }
    }
    fn fail(desc: impl Into<String>) -> Self {
        Self { desc: desc.into(), ok: false, skipped: false }
    }
    fn skip(desc: impl Into<String>) -> Self {
        Self { desc: desc.into(), ok: true, skipped: true }
    }
}

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

    // entities.json
    let entities_path = dev_ops.join("entities.json");
    if !entities_path.exists() {
        let content = serde_json::json!({ "projects": [] });
        match std::fs::write(&entities_path, serde_json::to_string_pretty(&content).unwrap_or_default()) {
            Ok(_) => log.push(WizardAction::ok("created: entities.json")),
            Err(e) => log.push(WizardAction::fail(format!("entities.json: {}", e))),
        }
    } else {
        log.push(WizardAction::skip("exists: entities.json"));
    }

    // tasks.md
    let tasks_path = dev_ops.join("tasks.md");
    if !tasks_path.exists() {
        let content = "# Tasks\n\n<!-- raios task format: - [ ] Task text @agent #ProjectName -->\n";
        match std::fs::write(&tasks_path, content) {
            Ok(_) => log.push(WizardAction::ok("created: tasks.md")),
            Err(e) => log.push(WizardAction::fail(format!("tasks.md: {}", e))),
        }
    } else {
        log.push(WizardAction::skip("exists: tasks.md"));
    }

    // mempalace.yaml
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

/// Create MASTER.md template if it doesn't exist.
pub fn exec_master(master_path: &Path, github_user: &str) -> Vec<WizardAction> {
    let mut log = Vec::new();

    if master_path.exists() {
        log.push(WizardAction::skip("exists: MASTER.md"));
        return log;
    }

    if let Some(parent) = master_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let content = master_template(github_user);
    match std::fs::write(master_path, content) {
        Ok(_) => log.push(WizardAction::ok(format!("created: {}", master_path.display()))),
        Err(e) => log.push(WizardAction::fail(format!("MASTER.md: {}", e))),
    }
    log
}

/// Set up Claude Code: CLAUDE.md template + MCP registration.
pub fn exec_claude(dev_ops: &Path, master_path: &Path) -> Vec<WizardAction> {
    let mut log = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();
    let claude_dir = home.join(".claude");
    let _ = std::fs::create_dir_all(&claude_dir);

    // CLAUDE.md
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

    // settings.json — register raios MCP
    let settings_path = claude_dir.join("settings.json");
    match register_mcp_claude(&settings_path) {
        Ok(true)  => log.push(WizardAction::ok("registered: raios MCP in ~/.claude/settings.json")),
        Ok(false) => log.push(WizardAction::skip("already registered: raios MCP")),
        Err(e)    => log.push(WizardAction::fail(format!("MCP register: {}", e))),
    }

    // rules/ directory
    let rules_dir = claude_dir.join("rules");
    let _ = std::fs::create_dir_all(&rules_dir);
    log.push(WizardAction::ok("created: ~/.claude/rules/"));

    // .agents/skills link info (create dir)
    let skills_dir = dev_ops.join(".agents").join("skills");
    let _ = std::fs::create_dir_all(&skills_dir);
    log.push(WizardAction::ok("created: .agents/skills/"));

    log
}

/// Set up Gemini CLI: GEMINI.md template.
pub fn exec_gemini(master_path: &Path) -> Vec<WizardAction> {
    let mut log = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();
    let gemini_dir = home.join(".gemini");
    let _ = std::fs::create_dir_all(&gemini_dir);

    let gemini_md = gemini_dir.join("GEMINI.md");
    if gemini_md.exists() {
        log.push(WizardAction::skip("exists: ~/.gemini/GEMINI.md"));
    } else {
        let content = gemini_md_template(master_path);
        match std::fs::write(&gemini_md, content) {
            Ok(_) => log.push(WizardAction::ok("created: ~/.gemini/GEMINI.md")),
            Err(e) => log.push(WizardAction::fail(format!("GEMINI.md: {}", e))),
        }
    }

    // GEMINI settings.json — MCP
    let gemini_settings = gemini_dir.join("settings.json");
    match register_mcp_gemini(&gemini_settings) {
        Ok(true)  => log.push(WizardAction::ok("registered: raios MCP in ~/.gemini/settings.json")),
        Ok(false) => log.push(WizardAction::skip("already registered: raios MCP (Gemini)")),
        Err(e)    => log.push(WizardAction::fail(format!("Gemini MCP: {}", e))),
    }

    log
}

/// Set up Antigravity: ANTIGRAVITY.md template.
pub fn exec_antigravity(dev_ops: &Path, master_path: &Path) -> Vec<WizardAction> {
    let mut log = Vec::new();
    let agents_dir = dev_ops.join(".agents");
    let _ = std::fs::create_dir_all(&agents_dir);

    let ag_md = agents_dir.join("ANTIGRAVITY.md");
    if ag_md.exists() {
        log.push(WizardAction::skip("exists: .agents/ANTIGRAVITY.md"));
    } else {
        let content = antigravity_md_template(master_path);
        match std::fs::write(&ag_md, content) {
            Ok(_) => log.push(WizardAction::ok("created: .agents/ANTIGRAVITY.md")),
            Err(e) => log.push(WizardAction::fail(format!("ANTIGRAVITY.md: {}", e))),
        }
    }

    log
}

/// Create .agents/skills/ and .agents/hooks/ with starter files.
pub fn exec_skills(dev_ops: &Path) -> Vec<WizardAction> {
    let mut log = Vec::new();
    let agents_dir = dev_ops.join(".agents");

    let dirs_to_create = [
        agents_dir.join("skills"),
        agents_dir.join("hooks"),
    ];

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

    // Starter skill stubs
    let skills_dir = agents_dir.join("skills");
    let skill_stubs = [
        ("prompt-master.md", SKILL_PROMPT_MASTER),
        ("graphify.md", SKILL_GRAPHIFY),
        ("verify-ai-os.md", SKILL_VERIFY),
        ("ki-snapshot.md", SKILL_KI_SNAPSHOT),
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

    // hooks README
    let hooks_readme = agents_dir.join("hooks").join("README.md");
    if !hooks_readme.exists() {
        let _ = std::fs::write(&hooks_readme, HOOKS_README);
        log.push(WizardAction::ok("created: hooks/README.md"));
    }

    log
}

/// Write config.toml and run initial discover.
pub fn exec_initialize(
    dev_ops: &Path,
    master_path: &Path,
    skills_path: &Path,
    vault_path: Option<&Path>,
) -> Vec<WizardAction> {
    let mut log = Vec::new();

    let config = crate::config::Config {
        dev_ops_path: dev_ops.to_path_buf(),
        master_md_path: master_path.to_path_buf(),
        skills_path: skills_path.to_path_buf(),
        vault_projects_path: vault_path.map(|p| p.to_path_buf()).unwrap_or_default(),
    };

    match config.save() {
        Ok(_) => log.push(WizardAction::ok("saved: ~/.config/raios/config.toml")),
        Err(e) => log.push(WizardAction::fail(format!("config.toml: {}", e))),
    }

    // Initial discover
    let projects = crate::entities::discover_entities(dev_ops);
    let count = projects.len();
    match crate::entities::save_entities(dev_ops, projects) {
        Ok(_) => log.push(WizardAction::ok(format!("discovered {} projects → entities.json", count))),
        Err(e) => log.push(WizardAction::fail(format!("discover: {}", e))),
    }

    log
}

// ─── MCP registration ─────────────────────────────────────────────────────────

fn register_mcp_claude(settings_path: &Path) -> Result<bool, String> {
    let mut json: serde_json::Value = if settings_path.exists() {
        let s = std::fs::read_to_string(settings_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&s).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Check if already registered
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

fn register_mcp_gemini(settings_path: &Path) -> Result<bool, String> {
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

// ─── Templates ────────────────────────────────────────────────────────────────

fn master_template(github_user: &str) -> String {
    format!(r#"# MASTER — {user}

---

## 1. Kimlik & Davranış

### Kimlik
Kişisel asistanın. Arkadaş gibi konuş, iş önce. Net, direkt, gereksiz uzatma yok.
Güvenlik ve performans odaklı pair-programming uzmanısın.

### Dil
- Kod: İngilizce
- İletişim: Türkçe

---

## 2. Kodlama Standartları

### Paket Yönetimi
pnpm > npm/yarn. Python: uv/pip.

### Kod Kuralları
1. Önce amacı netleştir → scope + edge case
2. Skeleton önce, dosyaları doldurmadan yapıyı onayla
3. Fonksiyonel yaz
4. Hata yönetimi her zaman
5. Yorum satırı yok, kod konuşsun

---

## 3. Güvenlik

- API key'ler asla client-side'da olmaz
- RLS day 0'dan
- Managed services kullan

---

## 4. Sistem & Süreç

### Proje Konumu
Tüm projeler `Dev Ops/` altında, istisna yok.

### Git Kuralları
- Commit mesajı: İngilizce, kısa, net
- Push etmeyi unutma

### Agent İş Bölümü
- **Claude Code:** İnteraktif geliştirme
- **Gemini CLI:** Araştırma ve alternatif mimari
- **Antigravity:** Görsel ve performans odaklı geliştirme

### GitHub
- Kullanıcı: {user}
"#, user = if github_user.is_empty() { "User" } else { github_user })
}

fn claude_md_template(master_path: &Path) -> String {
    format!(r#"# Claude Code Rules

> Full constitution: {}

## Quick Reference
- Code: English | Communication: Turkish (Türkçe)
- Package manager: pnpm > npm/yarn
- Security: API keys never client-side, RLS day 0
- No console.log in production — use logger utility
- TypeScript strict mode, no `any`

## Mandatory Skills (run before relevant tasks)
- `/prompt-master` — before any complex prompt
- `/graphify` — on codebase entry and analysis
- `/verify-ai-os` — session start and on inconsistency

## Agent Handover
Use `handover` MCP tool to pass tasks to Gemini or Antigravity.
"#, master_path.display())
}

fn gemini_md_template(master_path: &Path) -> String {
    format!(r#"# Gemini CLI Rules

> Full constitution: {}

## Role
Research, alternative architecture exploration, graphify analysis.

## Quick Reference
- Code: English | Communication: Turkish
- Always run graphify on codebase entry
- Use `handover` to pass implementation back to Claude

## Mandatory
- `graphify` motor: run on every research session
- Document findings in project memory.md
"#, master_path.display())
}

fn antigravity_md_template(master_path: &Path) -> String {
    format!(r#"# Antigravity Rules

> Full constitution: {}

## Role
Visual and performance-focused development. System health monitoring.

## Quick Reference
- Code: English | Communication: Turkish
- Glassmorphism UI standards: backdrop-blur-xl, bg-white/20
- Run `verify-ai-os` for system health checks
- Framer Motion for complex animations, tailwindcss-animate for simple

## Mandatory
- `verify-ai-os` on session start
- Monitor Core Web Vitals: LCP, FID, CLS
"#, master_path.display())
}

// ─── Skill stubs ──────────────────────────────────────────────────────────────

const SKILL_PROMPT_MASTER: &str = r#"---
name: prompt-master
description: Generates optimized prompts for any AI tool
type: skill
---

# prompt-master

Generates optimized prompts for LLMs, Cursor, Midjourney, coding agents.

## When to use
Before writing any complex prompt for an AI tool.

## Steps
1. Identify the target AI tool and its strengths
2. Define the task clearly: what in, what out
3. Add constraints: format, length, tone, style
4. Include examples if helpful
5. Test and iterate
"#;

const SKILL_GRAPHIFY: &str = r#"---
name: graphify
description: Convert any input to knowledge graph
type: skill
---

# graphify

Converts code, docs, papers, images to knowledge graph.

## When to use
- On codebase entry
- When analyzing complex systems
- Before major refactoring

## Steps
1. Read all key files in the project
2. Identify entities (modules, functions, data flows)
3. Map relationships between entities
4. Output as structured summary or graph
"#;

const SKILL_VERIFY: &str = r#"---
name: verify-ai-os
description: Verify all symbolic links, junctions, and rules across agents
type: skill
---

# verify-ai-os

System health check for the AI OS setup.

## When to use
- Session start
- On inconsistency or unexpected behavior
- After major system changes

## Checks
1. MASTER.md exists and readable
2. Agent configs (.claude, .gemini, .agents) present
3. entities.json valid
4. tasks.md readable
5. mempalace.yaml valid
"#;

const SKILL_KI_SNAPSHOT: &str = r#"---
name: ki-snapshot
description: Save session progress and context
type: skill
---

# ki-snapshot

Summarize and save progress at end of session or when context is large.

## When to use
- End of session
- Context getting too large
- Before handover to another agent

## Steps
1. Summarize what was accomplished
2. List remaining tasks
3. Note key decisions and why
4. Update memory.md with session summary
5. Commit if there are pending changes
"#;

const HOOKS_README: &str = r#"# Agent Hooks

Place shell scripts here to run on agent events.

## Format
Files named `<event>.sh` or `<event>.ps1` will be picked up by the hook system.

## Available Events
- `pre-tool-use` — before any tool call
- `post-tool-use` — after any tool call
- `session-start` — when agent session begins
- `session-end` — when agent session ends

## Example
```bash
#!/bin/bash
# post-tool-use.sh
echo "Tool used: $TOOL_NAME" >> ~/.raios/tool-log.txt
```
"#;
