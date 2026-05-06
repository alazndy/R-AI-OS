use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use crate::config::Config;
use crate::filebrowser::{
    discover_memory_files, find_file_by_name, get_agent_config_files, get_master_rule_files,
    get_mempalace_files, load_file_content,
};

#[derive(Parser)]
#[command(name = "raios", about = "AI OS Terminal Control Center — Rust Edition", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Output in JSON format
    #[arg(short, long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Print master rule files
    Rules {
        /// Filter by name
        name: Option<String>,
    },
    /// Print project memory.md files
    Memory {
        /// Project name filter
        project: Option<String>,
    },
    /// Print mempalace.yaml
    Mempalace,
    /// List all projects with memory.md
    Projects,
    /// List agent config files and their status
    Agents,
    /// View any known file by name or path
    View {
        /// File name or path
        name: String,
    },
    /// Run discovery engine to find new projects
    Discover,
    /// Get health report for a project (dirty, compliance, etc.)
    Health {
        /// Project name or path
        project: Option<String>,
    },
    /// Print current version
    Version,
    /// Run as MCP server (stdio transport — for Claude Code / Gemini integration)
    #[command(name = "mcp-server")]
    McpServer,
    /// Run an agent as a child process with execution proxy (env isolation, timeout)
    Run {
        /// The agent to run (claude, gemini, cursor)
        agent: String,
        /// Project directory to run the agent in
        #[arg(short, long)]
        project: Option<String>,
        /// Time limit in seconds for the agent execution
        #[arg(short, long)]
        timeout: Option<u64>,
    },
    /// Commit dirty projects in bulk (optionally push)
    Commit {
        /// Filter to a single project by name
        #[arg(short, long)]
        project: Option<String>,
        /// Custom commit message (default: "chore: raios auto-sync")
        #[arg(short, long)]
        message: Option<String>,
        /// Push after committing
        #[arg(long)]
        push: bool,
        /// Dry-run: show which projects would be committed without doing it
        #[arg(long)]
        dry_run: bool,
    },
    /// Show workspace portfolio statistics
    Stats,
    /// Scaffold a new project following MASTER.md rules
    New {
        /// Project name
        name: String,
        /// Category folder (e.g. "07_DevTools_&_Productivity")
        #[arg(short, long, default_value = "")]
        category: String,
        /// Create a private GitHub repo and push
        #[arg(long)]
        github: bool,
        /// Skip updating Vault Proje Atlası.md
        #[arg(long)]
        no_vault: bool,
    },
}

/// Load config or fall back to auto-detected dev_ops and a dummy master path.
fn load_cfg() -> Config {
    if let Some(cfg) = Config::load() {
        return cfg;
    }
    let detected = Config::auto_detect();
    Config {
        dev_ops_path:   detected.dev_ops.unwrap_or_else(|| {
            dirs::desktop_dir().unwrap_or_default().join("Dev Ops")
        }),
        master_md_path: detected.master_md.unwrap_or_else(|| PathBuf::from("MASTER.md")),
        skills_path:    detected.skills.unwrap_or_else(|| PathBuf::from(".agents/skills")),
        vault_projects_path: detected.vault_projects.unwrap_or_else(|| PathBuf::from("Projeler")),
    }
}

pub fn run(cli: Cli) {
    let cfg = load_cfg();
    let cmd = cli.command.unwrap(); // We know it exists
    match cmd {
        Commands::Rules { name }       => cmd_rules(name, &cfg.master_md_path, cli.json),
        Commands::Memory { project }   => cmd_memory(project, &cfg.dev_ops_path, cli.json),
        Commands::Mempalace            => cmd_mempalace(&cfg.dev_ops_path, cli.json),
        Commands::Projects             => cmd_projects(&cfg.dev_ops_path, cli.json),
        Commands::Agents               => cmd_agents(cli.json),
        Commands::View { name }        => cmd_view(name, &cfg.master_md_path, cli.json),
        Commands::Discover             => cmd_discover(&cfg.dev_ops_path, cli.json),
        Commands::Health { project }   => cmd_health(project, &cfg.dev_ops_path, cli.json),
        Commands::Version              => println!("raios v{}", env!("CARGO_PKG_VERSION")),
        Commands::McpServer            => {
            if let Err(e) = crate::mcp_server::run_stdio() {
                eprintln!("MCP server error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Run { agent, project, timeout } => {
            if let Err(e) = crate::agent_runner::run_agent(&agent, project, timeout) {
                eprintln!("Agent Runner Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Commit { project, message, push, dry_run } => {
            cmd_commit(project, message, push, dry_run, &cfg.dev_ops_path, cli.json);
        }
        Commands::Stats => {
            cmd_stats(&cfg.dev_ops_path, cli.json);
        }
        Commands::New { name, category, github, no_vault } => {
            cmd_new(&name, &category, github, no_vault, &cfg.dev_ops_path, cli.json);
        }
    }
}

fn cmd_rules(filter: Option<String>, master_md: &Path, json: bool) {
    let files = get_master_rule_files(master_md);
    let filter = filter.as_deref().map(str::to_lowercase);
    let mut results = Vec::new();

    for entry in &files {
        if let Some(ref f) = filter {
            if !entry.name.to_lowercase().contains(f.as_str()) {
                continue;
            }
        }
        if json {
            results.push(entry);
        } else {
            println!("=== {} ===", entry.name);
            println!("{}", load_file_content(&entry.path));
            println!();
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&results).unwrap());
    }
}

fn cmd_memory(project: Option<String>, dev_ops: &std::path::Path, json: bool) {
    let files = discover_memory_files(dev_ops, 100);
    let mut results = Vec::new();

    if let Some(query) = project {
        let q = query.to_lowercase();
        for m in files {
            if m.name.to_lowercase().contains(&q) {
                if json {
                    results.push(m);
                } else {
                    println!("=== {} ===", m.name);
                    println!("{}", load_file_content(&m.path));
                }
                break;
            }
        }
    } else {
        for m in files.into_iter().take(5) {
            if json {
                results.push(m);
            } else {
                println!("=== {} ===", m.name);
                println!("{}", load_file_content(&m.path));
                println!();
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results).unwrap());
    }
}

fn cmd_mempalace(dev_ops: &std::path::Path, json: bool) {
    let files = get_mempalace_files(dev_ops);
    if let Some(first) = files.first() {
        if json {
            println!("{}", serde_json::to_string_pretty(&first).unwrap());
        } else {
            println!("{}", load_file_content(&first.path));
        }
    }
}

fn cmd_projects(dev_ops: &std::path::Path, json: bool) {
    let files = discover_memory_files(dev_ops, 100);
    if json {
        println!("{}", serde_json::to_string_pretty(&files).unwrap());
    } else {
        for entry in files {
            let proj = entry.name.trim_end_matches("/memory.md");
            println!("• {:<40} {}", proj, entry.path.display());
        }
    }
}

fn cmd_agents(json: bool) {
    let entries = get_agent_config_files();
    if json {
        println!("{}", serde_json::to_string_pretty(&entries).unwrap());
    } else {
        for entry in entries {
            let mark = if entry.exists() { "✓" } else { "✗" };
            println!("[{}] {:<30} {}", mark, entry.name, entry.path.display());
        }
    }
}

fn cmd_view(name: String, master_md: &Path, json: bool) {
    match find_file_by_name(&name, master_md) {
        Some(entry) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&entry).unwrap());
            } else {
                println!("{}", load_file_content(&entry.path));
            }
        }
        None => {
            if json {
                println!("{{ \"error\": \"File not found\" }}");
            } else {
                eprintln!("File not found: {}", name);
            }
            std::process::exit(1);
        }
    }
}

fn cmd_discover(dev_ops: &std::path::Path, json: bool) {
    let projects = crate::entities::discover_entities(dev_ops);
    let _ = crate::entities::save_entities(dev_ops, projects.clone());
    if json {
        println!("{}", serde_json::to_string_pretty(&projects).unwrap());
    } else {
        println!("Discovery complete. Found {} projects.", projects.len());
    }
}

fn cmd_health(project: Option<String>, dev_ops: &std::path::Path, json: bool) {
    let projects = crate::entities::load_entities(dev_ops);
    let mut results = Vec::new();

    if let Some(q) = project {
        let query = q.to_lowercase();
        for p in &projects {
            if p.name.to_lowercase().contains(&query) || p.local_path.to_string_lossy().to_lowercase().contains(&query) {
                let report = crate::health::check_project(p);
                results.push(report);
            }
        }
    } else {
        for p in &projects {
            let report = crate::health::check_project(p);
            results.push(report);
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results).unwrap());
    } else {
        for r in results {
            let dirty = match r.git_dirty {
                Some(true) => "DIRTY",
                Some(false) => "CLEAN",
                None => "N/A",
            };
            let remote = r.remote_url.unwrap_or_else(|| "N/A".to_string());
            println!("Project: {:<20} | Status: {:<10} | Git: {:<5} | Grade: {} | URL: {}", r.name, r.status, dirty, r.compliance_grade, remote);
        }
    }
}

fn cmd_commit(
    project: Option<String>,
    message: Option<String>,
    push: bool,
    dry_run: bool,
    dev_ops: &std::path::Path,
    json: bool,
) {
    use crate::filebrowser::{git_commit, git_is_dirty, git_push};

    let projects = crate::entities::load_entities(dev_ops);
    let commit_msg = message.as_deref().unwrap_or("chore: raios auto-sync");

    let candidates: Vec<_> = if let Some(q) = project {
        let q = q.to_lowercase();
        projects.into_iter()
            .filter(|p| p.name.to_lowercase().contains(&q))
            .collect()
    } else {
        projects
    };

    #[derive(serde::Serialize)]
    struct CommitEntry { name: String, committed: bool, pushed: bool, note: String }
    let mut entries: Vec<CommitEntry> = Vec::new();
    let mut committed_count = 0usize;
    let mut skipped_count = 0usize;

    for p in &candidates {
        let dirty = git_is_dirty(&p.local_path).unwrap_or(false);
        if !dirty {
            skipped_count += 1;
            if !json { println!("  skip  {}", p.name); }
            continue;
        }
        if dry_run {
            if !json { println!("  would commit  {}", p.name); }
            entries.push(CommitEntry { name: p.name.clone(), committed: false, pushed: false, note: "dry-run".into() });
            continue;
        }
        let result = git_commit(&p.local_path, commit_msg);
        let mut pushed_ok = false;
        let mut note = result.message.clone();
        if result.committed && push {
            match git_push(&p.local_path) {
                Ok(()) => { pushed_ok = true; note = "committed + pushed".into(); }
                Err(e) => { note = format!("committed, push failed: {}", e); }
            }
        } else if result.committed {
            note = "committed".into();
        }
        if result.committed { committed_count += 1; } else { skipped_count += 1; }
        if !json {
            let status = if result.committed { if pushed_ok { "  ✓ push " } else { "  ✓ commit" } } else { "  - skip  " };
            println!("{} {} — {}", status, p.name, note);
        }
        entries.push(CommitEntry { name: p.name.clone(), committed: result.committed, pushed: pushed_ok, note });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&entries).unwrap_or_default());
    } else {
        println!("\nDone — {} committed, {} skipped.", committed_count, skipped_count);
    }
}

fn cmd_stats(dev_ops: &std::path::Path, json: bool) {
    use std::collections::HashMap;

    let projects = crate::entities::load_entities(dev_ops);
    let total = projects.len();

    let mut active = 0usize;
    let mut archived = 0usize;
    let mut dirty = 0usize;
    let mut no_memory = 0usize;
    let mut local_only = 0usize;
    let mut grade_a = 0usize;
    let mut grade_b = 0usize;
    let mut grade_c = 0usize;
    let mut grade_d = 0usize;
    let mut category_counts: HashMap<String, usize> = HashMap::new();

    for p in &projects {
        match p.status.as_str() {
            "active" => active += 1,
            "archived" | "legacy" => archived += 1,
            _ => active += 1,
        }
        if p.github.is_none() { local_only += 1; }
        if crate::filebrowser::git_is_dirty(&p.local_path) == Some(true) { dirty += 1; }
        if !p.local_path.join("memory.md").exists() { no_memory += 1; }
        let health = crate::health::check_project(p);
        match health.compliance_grade.as_str() {
            "A" => grade_a += 1,
            "B" => grade_b += 1,
            "C" => grade_c += 1,
            _ => grade_d += 1,
        }
        *category_counts.entry(p.category.clone()).or_insert(0) += 1;
    }

    if json {
        #[derive(serde::Serialize)]
        struct Stats {
            total: usize, active: usize, archived: usize,
            dirty: usize, no_memory: usize, local_only: usize,
            grade_a: usize, grade_b: usize, grade_c: usize, grade_d: usize,
            categories: HashMap<String, usize>,
        }
        let s = Stats { total, active, archived, dirty, no_memory, local_only,
            grade_a, grade_b, grade_c, grade_d, categories: category_counts };
        println!("{}", serde_json::to_string_pretty(&s).unwrap_or_default());
        return;
    }

    fn bar(n: usize, total: usize, width: usize) -> String {
        if total == 0 { return String::new(); }
        let filled = (n * width) / total;
        "█".repeat(filled)
    }

    println!("Portfolio Statistics — R-AI-OS v{}", env!("CARGO_PKG_VERSION"));
    println!("{}", "─".repeat(46));
    println!("Total projects:      {:>5}", total);
    println!("Active / Archived:   {:>5} / {}", active, archived);
    println!("Dirty (uncommitted): {:>5}", dirty);
    println!("No memory.md:        {:>5}", no_memory);
    println!("Local only (no GH):  {:>5}", local_only);
    println!();
    println!("Grade Distribution:");
    println!("  A (≥80): {:>4} projects  {} {}%",
        grade_a, bar(grade_a, total, 24),
        if total > 0 { grade_a * 100 / total } else { 0 });
    println!("  B (≥60): {:>4} projects  {} {}%",
        grade_b, bar(grade_b, total, 24),
        if total > 0 { grade_b * 100 / total } else { 0 });
    println!("  C (≥40): {:>4} projects  {} {}%",
        grade_c, bar(grade_c, total, 24),
        if total > 0 { grade_c * 100 / total } else { 0 });
    println!("  D  (<40): {:>4} projects  {} {}%",
        grade_d, bar(grade_d, total, 24),
        if total > 0 { grade_d * 100 / total } else { 0 });
    println!();
    println!("Top Categories:");
    let mut cats: Vec<_> = category_counts.iter().collect();
    cats.sort_by(|a, b| b.1.cmp(a.1));
    for (cat, count) in cats.iter().take(8) {
        let display = cat.replace('_', " ");
        println!("  {:<28} {}", display, count);
    }
}

fn cmd_new(name: &str, category: &str, github: bool, no_vault: bool, dev_ops: &std::path::Path, json: bool) {
    let effective_category = if category.is_empty() { "Uncategorized" } else { category };
    let cfg = crate::new_project::NewProjectConfig {
        name,
        category: effective_category,
        dev_ops,
        github,
        no_vault,
    };
    let result = crate::new_project::create(&cfg);

    if json {
        #[derive(serde::Serialize)]
        struct Out { path: String, github_url: Option<String>, steps: Vec<(String, bool)> }
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
    let all_ok = result.steps.iter().all(|(_, ok)| *ok);
    if all_ok {
        println!("Done. Project ready at {}", result.path.display());
    } else {
        println!("Completed with some errors. Check the steps above.");
    }
}
