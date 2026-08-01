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
        vault_path: None,
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

pub(super) fn cmd_bootstrap(cfg: &raios_core::config::BootstrapConfig, dry_run: bool, yes: bool) {
    let plan = raios_runtime::bootstrap::build_plan(cfg);

    if plan.is_empty() {
        println!("Nothing configured — see [bootstrap] in ~/.config/raios/config.toml");
        return;
    }

    println!("raios bootstrap plan ({} action(s)):", plan.len());
    for (i, action) in plan.iter().enumerate() {
        println!("  {}. {}", i + 1, action.describe());
    }
    println!();

    if dry_run {
        println!("(dry run — nothing executed)");
        return;
    }

    if !yes {
        print!("Proceed with {} action(s)? [y/N] ", plan.len());
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err()
            || !matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
        {
            println!("Aborted.");
            return;
        }
    }

    let results = raios_runtime::bootstrap::execute(&plan);

    println!();
    let mut failed = 0;
    for (desc, outcome) in &results {
        match outcome {
            raios_runtime::bootstrap::ActionOutcome::Ok => println!("  [ok] {desc}"),
            raios_runtime::bootstrap::ActionOutcome::Skipped(reason) => {
                println!("  [skipped: {reason}] {desc}")
            }
            raios_runtime::bootstrap::ActionOutcome::Failed(reason) => {
                println!("  [failed: {reason}] {desc}");
                failed += 1;
            }
        }
    }
    println!();
    if failed == 0 {
        println!("Bootstrap complete.");
    } else {
        println!("Bootstrap completed with {failed} failure(s).");
        std::process::exit(1);
    }
}
