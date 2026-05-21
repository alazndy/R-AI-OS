use std::path::{Path, PathBuf};

use super::InstinctCmd;

pub(super) fn cmd_instinct(cmd: InstinctCmd, dev_ops: &Path, json: bool) {
    match cmd {
        InstinctCmd::Add { rule, path } => add(&rule, path, json),
        InstinctCmd::List { path } => list(path, json),
        InstinctCmd::Suggest { project } => suggest(project, dev_ops, json),
    }
}

fn add(rule: &str, path: Option<PathBuf>, json: bool) {
    use crate::instinct::{append_to_memory_md, InstinctEngine};

    let mut engine = InstinctEngine::init();
    engine.add_rule(rule.to_string());
    if let Err(e) = engine.save() {
        eprintln!("Failed to save instinct: {e}");
        return;
    }

    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let memory_ok = match append_to_memory_md(&project_path, rule) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("Warning: memory.md write failed: {e}");
            false
        }
    };

    if json {
        println!(
            "{}",
            serde_json::json!({"status":"ok","rule":rule,"memory_written":memory_ok})
        );
    } else if memory_ok {
        println!("Saved to ~/.agents/instincts.json");
        println!("Appended to {}/memory.md", project_path.display());
    } else {
        println!("Saved to ~/.agents/instincts.json only");
    }
}

fn list(path: Option<PathBuf>, json: bool) {
    use crate::instinct::{load_project_rules, InstinctEngine};

    let engine = InstinctEngine::init();
    let global = &engine.data.learned_rules;
    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let project = load_project_rules(&project_path);

    if json {
        let out = serde_json::json!({"global": global, "project": project});
        match serde_json::to_string_pretty(&out) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
        return;
    }

    println!("Global instincts ({}):", global.len());
    if global.is_empty() {
        println!("  (none)");
    } else {
        for r in global {
            println!("  - {r}");
        }
    }
    println!("\nProject instincts ({}):", project.len());
    if project.is_empty() {
        println!("  (none)");
    } else {
        for r in &project {
            println!("  - {r}");
        }
    }
}

fn suggest(project: Option<String>, dev_ops: &Path, _json: bool) {
    use crate::health::check_project;
    use crate::instinct::{append_to_memory_md, suggest_from_health, InstinctEngine};

    let projects = crate::entities::load_entities(dev_ops);
    let target = if let Some(ref name) = project {
        let n = name.to_lowercase();
        projects.into_iter().find(|p| p.name.to_lowercase().contains(&n))
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        projects.into_iter().find(|p| p.local_path == cwd)
    };

    let proj = match target {
        Some(p) => p,
        None => {
            eprintln!("Project not found. Try: raios instinct suggest <project-name>");
            std::process::exit(1);
        }
    };

    println!("Analyzing project: {}...", proj.name);
    let health = check_project(&proj);
    let suggestions = suggest_from_health(&health);

    if suggestions.is_empty() {
        println!("No suggestions — project looks healthy!");
        return;
    }

    println!("\nSuggested instincts:");
    for (i, s) in suggestions.iter().enumerate() {
        println!("  [{}] {}", i + 1, s);
    }

    print!("\nAccept? (y=all / 1,2=specific / n=none): ");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        eprintln!("Could not read input");
        return;
    }
    let input = input.trim().to_lowercase();

    let accepted: Vec<&String> = if input == "y" {
        suggestions.iter().collect()
    } else if input == "n" || input.is_empty() {
        vec![]
    } else {
        input
            .split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .filter(|&i| i >= 1 && i <= suggestions.len())
            .map(|i| &suggestions[i - 1])
            .collect()
    };

    if accepted.is_empty() {
        println!("No instincts added.");
        return;
    }

    let mut engine = InstinctEngine::init();
    for rule in &accepted {
        engine.add_rule((*rule).clone());
        match append_to_memory_md(&proj.local_path, rule) {
            Ok(()) => println!("Saved: \"{}\"", rule),
            Err(e) => {
                eprintln!("Warning: memory.md: {e}");
                println!("JSON only: \"{}\"", rule);
            }
        }
    }
    if let Err(e) = engine.save() {
        eprintln!("Failed to save instincts.json: {e}");
    }
}
