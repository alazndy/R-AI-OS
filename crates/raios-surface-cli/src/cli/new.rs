use std::path::Path;

pub(super) fn cmd_new(
    name: &str,
    category: &str,
    github: bool,
    no_vault: bool,
    dev_ops: &Path,
    json: bool,
) {
    let effective_category = if category.is_empty() {
        "Uncategorized"
    } else {
        category
    };
    let cfg = raios_runtime::new_project::NewProjectConfig {
        name,
        category: effective_category,
        dev_ops,
        github,
        no_vault,
    };
    let result = raios_runtime::new_project::create(&cfg);

    if json {
        #[derive(serde::Serialize)]
        struct Out {
            path: String,
            github_url: Option<String>,
            steps: Vec<(String, bool)>,
        }
        let out = Out {
            path: result.path.display().to_string(),
            github_url: result.github_url,
            steps: result.steps,
        };
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    println!("Project: {}", name);
    println!("Path:    {}", result.path.display());
    if let Some(url) = &result.github_url {
        println!("GitHub:  {}", url);
    }
    println!();
    for (desc, ok) in &result.steps {
        println!("  [{}] {}", if *ok { "✓" } else { "✗" }, desc);
    }
    println!();
    if result.steps.iter().all(|(_, ok)| *ok) {
        println!("Done. Project ready at {}", result.path.display());
    } else {
        println!("Completed with some errors. Check the steps above.");
    }
}

pub(super) fn cmd_task(
    description: &str,
    project_dir: Option<String>,
    force_agent: Option<String>,
) {
    use raios_runtime::router::AgentRouter;
    println!("Routing task: {}", description);

    let agent = if let Some(a) = force_agent {
        println!("Manual agent override: {}", a);
        a
    } else {
        let router = AgentRouter::init().expect("Failed to init AgentRouter");
        match router.route(description) {
            Ok(Some(a)) => {
                println!("Best specialist found: {}", a);
                a
            }
            Ok(None) => {
                println!("No specific specialist found.");
                "claude".to_string()
            }
            Err(e) => {
                eprintln!("Routing error: {}.", e);
                "claude".to_string()
            }
        }
    };

    println!("Invoking {} with the task...", agent);
    let _ = raios_runtime::agent_runner::run_agent(&agent, project_dir, None, vec![]);
}

pub(super) fn cmd_bootstrap() {
    println!("Starting Raios TOTAL SYSTEM BOOTSTRAP...");

    let is_windows = cfg!(target_os = "windows");
    let home_dir = dirs::home_dir().expect("Could not find home directory");
    let temp_dir = std::env::temp_dir();

    println!("--- [1/5] Checking Global CLI Ecosystem ---");
    for tool in ["sigmap", "ctx7", "vercel", "firebase-tools"] {
        let check_cmd = if is_windows { "where" } else { "which" };
        let status = std::process::Command::new(check_cmd)
            .arg(tool)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if status.is_err() || !status.unwrap().success() {
            println!("Installing {} globally via npm...", tool);
            let _ = std::process::Command::new("npm")
                .args(["install", "-g", tool])
                .status();
        } else {
            println!("✓ {} is already installed.", tool);
        }
    }

    println!("--- [2/5] Configuring Claude Code Plugins ---");
    for args in [
        vec![
            "plugin",
            "marketplace",
            "add",
            "https://github.com/josstei/maestro-orchestrate.git",
        ],
        vec![
            "plugin",
            "marketplace",
            "add",
            "https://github.com/affaan-m/everything-claude-code.git",
        ],
        vec![
            "plugin",
            "install",
            "maestro@maestro-orchestrator",
            "--scope",
            "user",
        ],
        vec![
            "plugin",
            "install",
            "everything-claude-code@everything-claude-code",
            "--scope",
            "user",
        ],
    ] {
        let _ = std::process::Command::new("claude").args(&args).status();
    }

    println!("--- [4/5] Syncing ECC Skills & Rules ---");
    let ecc_temp_path = temp_dir.join("ecc-master");
    if !ecc_temp_path.exists() {
        let _ = std::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "https://github.com/affaan-m/everything-claude-code.git",
                ecc_temp_path.to_string_lossy().as_ref(),
            ])
            .status();
    } else {
        let _ = std::process::Command::new("git")
            .current_dir(&ecc_temp_path)
            .args(["pull"])
            .status();
    }

    let claude_rules = home_dir.join(".claude").join("rules");
    let antigravity_rules = home_dir.join(".antigravity").join("rules");
    let opencode_dir = home_dir.join(".config").join("opencode");
    for d in [&claude_rules, &antigravity_rules, &opencode_dir] {
        let _ = std::fs::create_dir_all(d);
    }

    let copied = copy_dir_recursive(&ecc_temp_path.join("rules"), &claude_rules);
    copy_dir_recursive(&ecc_temp_path.join("rules"), &antigravity_rules);
    println!("    Synced {copied} rule file(s) from everything-claude-code.");

    println!("--- [5/5] Final Touches & Activations ---");
    let master_path = home_dir
        .join("Documents")
        .join("Obsidian Vaults")
        .join("Vault101")
        .join("MASTER.md");
    if !master_path.exists() {
        if let Some(parent) = master_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&master_path, DEFAULT_MASTER_MD);
        println!("✓ Writing default MASTER.md to {}", master_path.display());
    }

    for plugin in [
        "superpowers@claude-plugins-official",
        "context7@claude-plugins-official",
        "frontend-design@claude-plugins-official",
        "github@claude-plugins-official",
    ] {
        let _ = std::process::Command::new("claude")
            .args(["plugin", "enable", plugin])
            .status();
    }

    println!("\nBOOTSTRAP COMPLETE: Your AI OS Factory is fully operational!");
    println!(
        "Synced {copied} skill/rule file(s). Run `raios agents` to see registered agent configs."
    );
}

/// Returns the number of files actually copied, so callers can report a
/// real count instead of a guessed or hardcoded one.
fn copy_dir_recursive(src: &Path, dst: &Path) -> usize {
    use walkdir::WalkDir;
    let mut copied = 0;
    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let destination = dst.join(path.strip_prefix(src).expect("Path stripping failed"));
        if path.is_dir() {
            let _ = std::fs::create_dir_all(&destination);
        } else if std::fs::copy(path, &destination).is_ok() {
            copied += 1;
        }
    }
    copied
}

const DEFAULT_MASTER_MD: &str = r#"# MASTER — Goktug

## 1. Identity & Behavior
You are Goktug's personal assistant. Speak like a work friend. Code: English | Communication: Turkish.

## 2. Coding Standards
pnpm > npm/yarn. Python: uv/pip. Write functionally. Error handling always. No comment lines.

## 3. Security
API keys never client-side. RLS day 0. Managed services preferred.

## 4. System & Process
All projects under Dev_Ops_New/, no exceptions.

## 5. Agent System
Claude Code: interactive dev | Antigravity: IDE dev.
"#;
