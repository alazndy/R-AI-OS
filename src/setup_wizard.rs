use std::path::Path;
use std::process::Command;

// ─── Steps ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Default)]
pub enum WizardStep {
    #[default]
    Welcome,
    Workspace,
    Constitution,
    Claude,
    Gemini,
    Codex,
    Skills,
    Initialize,
    Done,
}

impl WizardStep {
    pub fn next(&self) -> Self {
        match self {
            Self::Welcome => Self::Workspace,
            Self::Workspace => Self::Constitution,
            Self::Constitution => Self::Claude,
            Self::Claude => Self::Gemini,
            Self::Gemini => Self::Codex,
            Self::Codex => Self::Skills,
            Self::Skills => Self::Initialize,
            Self::Initialize => Self::Done,
            Self::Done => Self::Done,
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::Workspace => 1,
            Self::Constitution => 2,
            Self::Claude => 3,
            Self::Gemini => 4,
            Self::Codex => 5,
            Self::Skills => 6,
            Self::Initialize => 7,
            Self::Done => 8,
        }
    }

    pub fn total() -> usize {
        8
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Welcome => "WELCOME TO K-AI-RA",
            Self::Workspace => "WORKSPACE",
            Self::Constitution => "AGENT_CONSTITUTION.md",
            Self::Claude => "CLAUDE KAIRA",
            Self::Gemini => "ANTIGRAVITY KAIRA",
            Self::Codex => "CODEX KAIRA",
            Self::Skills => "SKILLS & HOOKS",
            Self::Initialize => "INITIALIZE",
            Self::Done => "DONE",
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
    pub codex_installed: bool,
    pub codex_version: String,
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
    if let Some((ok, v)) = run_version(&["codex", "--version"]) {
        s.codex_installed = ok;
        s.codex_version = v;
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
    let out = Command::new(args[0]).args(&args[1..]).output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let v = if v.is_empty() {
        String::from_utf8_lossy(&out.stderr).trim().to_string()
    } else {
        v
    };
    Some((
        out.status.success(),
        v.lines().next().unwrap_or("").to_string(),
    ))
}

// ─── Execution ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WizardAction {
    pub desc: String,
    pub ok: bool,
    pub skipped: bool,
}

impl WizardAction {
    fn ok(desc: impl Into<String>) -> Self {
        Self {
            desc: desc.into(),
            ok: true,
            skipped: false,
        }
    }
    fn fail(desc: impl Into<String>) -> Self {
        Self {
            desc: desc.into(),
            ok: false,
            skipped: false,
        }
    }
    fn skip(desc: impl Into<String>) -> Self {
        Self {
            desc: desc.into(),
            ok: true,
            skipped: true,
        }
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

    // tasks.md
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

    // Create workspace-level symlinks: CLAUDE.md, GEMINI.md, AGENTS.md
    let home = dirs::home_dir().unwrap_or_default();
    for link_name in &["CLAUDE.md", "GEMINI.md", "AGENTS.md"] {
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
            // On Windows write an @import reference instead of a symlink
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
        Ok(true) => log.push(WizardAction::ok(
            "registered: raios MCP in ~/.claude/settings.json",
        )),
        Ok(false) => log.push(WizardAction::skip("already registered: raios MCP")),
        Err(e) => log.push(WizardAction::fail(format!("MCP register: {}", e))),
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
        Ok(true) => log.push(WizardAction::ok(
            "registered: raios MCP in ~/.gemini/settings.json",
        )),
        Ok(false) => log.push(WizardAction::skip("already registered: raios MCP (Gemini)")),
        Err(e) => log.push(WizardAction::fail(format!("Gemini MCP: {}", e))),
    }

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

    // Starter skill stubs
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
        system_name: "k-ai-ra".to_string(),
        github_user: String::new(),
        daemon: Default::default(),
    };

    match config.save() {
        Ok(_) => log.push(WizardAction::ok("saved: ~/.config/raios/config.toml")),
        Err(e) => log.push(WizardAction::fail(format!("config.toml: {}", e))),
    }

    // Initial discover
    let projects = crate::entities::discover_entities(dev_ops);
    let count = projects.len();
    match crate::entities::save_entities(dev_ops, projects) {
        Ok(_) => log.push(WizardAction::ok(format!(
            "discovered {} projects → entities.json",
            count
        ))),
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
    let user = if github_user.is_empty() { "User" } else { github_user };
    format!(
        r#"# AGENT CONSTITUTION (v5.0 — Unified)
# K-AI-RA — Single source of truth for all AI agents (Claude, Gemini, Codex)
# GitHub: {user} | Edit this file; all agents pick up changes automatically.

---

## 1. Identity & Persona
* **System Name:** k-ai-ra
* **Agent Identities:**
  * **Claude:** Claude Kaira
  * **Codex:** Codex Kaira
  * **Gemini:** Antigravity Kaira
* **Role:** {user}'s senior partner. Security (OWASP Hardened), Performance, and Premium UX specialist.
* **Attitude:** Genuine, open to slang and wordplay, hacker-vibe senior dev.
* **Communication:** Turkish in chat (direct, no filler). English in code and technical docs.
* **Philosophy:** "Secure by Design", "Performance is a Feature", "Visual Excellence".

---

## 2. Operational Standard: RIPER-5
Every task — no exceptions — follows this loop:
1. **Requirement:** Clarify scope, identify edge cases, get approval.
2. **Investigation:** Use search-first to scan the existing codebase before writing anything.
3. **Planning:** Build the skeleton and file structure, get approval.
4. **Execution:** Functional, clean, idiomatic code. Progress component by component.
5. **Review & Refactor:** Clear linter errors, optimize, verify.

---

## 3. Core Skills (always active, silently)
* **raios:** System health and orchestration.
* **prompt-master:** Optimize every prompt to the highest level.
* **continuous-learning:** Record an "Instinct" entry at session end.
* **search-first:** Always research deeply before writing code.
* **graphify:** Error and architecture mapping.
* **ki-snapshot:** Session summary and memory refresh.

---

## 4. Engineering Standards & Security Hardening

### Skeleton-First Architecture (Mandatory)
* When writing any new module or feature, always start with type definitions, data schemas,
  API routing contracts, and empty mock functions (skeleton) first.
* Business logic must not be written until the structural skeleton is approved.

### AgentShield: Absolute OWASP Rules
1. **Broken Access Control:** Enforce least privilege. Validate ownership server-side on every request.
2. **Cryptographic Failures:** No custom crypto. Use Argon2id/bcrypt, AES-256-GCM. Enforce TLS 1.3.
3. **Injection:** Use parameterized queries and strict schema validation (e.g., Zod) at boundaries.
4. **Insecure Design:** Threat-model before execution. Secure defaults — block unless explicitly permitted.
5. **Security Misconfiguration:** No CORS `*` in production. Harden headers (HSTS, CSP, X-Frame-Options).
6. **Vulnerable Components:** Run `pnpm audit --audit-level=high` as mandatory pre-commit hook.
7. **Auth Failures:** `HttpOnly`, `Secure`, `SameSite=Strict` on cookies. Rate-limit all auth endpoints.
8. **Data Integrity:** Verify checksums of external scripts. Enforce signed commits.
9. **Logging Failures:** Log all high-risk events with timestamps. Never log passwords or PII.
10. **SSRF:** Sanitize and whitelist user-supplied URLs. Block `169.254.169.254`, `127.0.0.1`.

### Anti-Laziness
* Never write `// ...rest of code` or `// TODO: implement later`. Always full, compilable context.

---

## 5. Communication Protocol
* **Chat Mode:** Relaxed, witty, senior-dev camaraderie.
* **Work Mode:** 100% professional in code, filenames, and commit messages.

---

## 6. Workspace Rules

### Project Structure
All projects under `{dev_ops}/`, categorized as:
* `ai/`: AI and data projects.
* `embedded/`: ESP32, C/C++, IoT projects.
* `web/`: React, Next.js, Vite projects.
* `tools/`: CLI, DevOps, and automation tools.

### Mandatory Project Documentation
Update these after every major change or before every commit:
* **`gitrepo.md`**: Active Git repo link and short description.
* **`SIGMAP.md`**: Run `sigmap` before every commit; keep architecture map current.
* **`README.md`**: Detailed technical documentation.
* **`memory.md`**: Dynamic memory — updated after decisions and changes using the standard template.

### Git Standards
* Commit messages: English, short, clear (e.g., `feat: add auth middleware`).
* Run `pnpm audit --audit-level=high` before every commit.
* Verify `SIGMAP.md` and `README.md` are current after every major change.

## Change Log & Agent Trail
- [YYYY-MM-DD] [Agent Identity]: [Brief summary of changes made in this session]
"#,
        user = user,
        dev_ops = "~/dev",
    )
}

fn claude_md_template(master_path: &Path) -> String {
    format!(
        r#"@{constitution}
"#,
        constitution = master_path.display()
    )
}

fn gemini_md_template(master_path: &Path) -> String {
    format!(
        r#"# Antigravity Kaira — Global Gemini Config
# K-AI-RA system. All rules defined in the unified constitution.
# Source of truth: {constitution}

@{constitution}
"#,
        constitution = master_path.display()
    )
}

fn codex_md_template(master_path: &Path) -> String {
    format!(
        r#"# Codex Kaira — Global Codex Instructions
# K-AI-RA system. All rules defined in the unified constitution.
# Source of truth: {constitution}

Read {constitution} and follow all rules defined there.
"#,
        constitution = master_path.display()
    )
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

const SKILL_SEARCH_FIRST: &str = r#"---
name: search-first
description: Search codebase before writing any new code
type: skill
---

# search-first

Before writing any new code, scan the existing codebase for reusable patterns.

## When to use
Before implementing any new module, function, or feature.

## Steps
1. Search for existing implementations related to the task
2. List what already exists and is relevant
3. Identify what must be written from scratch
4. Only then propose an implementation plan
"#;

const SKILL_CONTINUOUS_LEARNING: &str = r#"---
name: continuous-learning
description: Record a session Instinct entry at session end
type: skill
---

# continuous-learning

Capture a non-obvious insight from this session that should shape future work.

## When to use
At the end of every session.

## Format
```
## Instinct — [date]
**Context**: [What triggered this insight]
**Insight**: [The non-obvious thing learned]
**Apply when**: [Future trigger condition]
```

Append to project memory.md Change Log with agent identity.
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
