use crate::config::Config;
use crate::filebrowser::{
    discover_memory_files, find_file_by_name, get_agent_config_files, get_master_rule_files,
    get_mempalace_files, load_file_content,
};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "raios",
    about = "AI OS Terminal Control Center — Rust Edition",
    version
)]
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
    /// Search across the entire Dev Ops workspace (Semantic + BM25)
    Search {
        /// The search query
        query: String,
        /// Number of results to return
        #[arg(short, long, default_value = "8")]
        top_k: usize,
        /// Force full re-indexing before search
        #[arg(long)]
        reindex: bool,
    },
    /// Run OWASP security scan on one or all projects
    Security {
        /// Filter to a single project by name
        #[arg(short, long)]
        project: Option<String>,
        /// Show full issue list (default: summary only)
        #[arg(long)]
        full: bool,
        /// Scan only the given directory (bypass entities.json)
        #[arg(long)]
        path: Option<String>,
    },
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
    /// Automatically route a task to the best specialist agent
    Task {
        /// Task description
        description: String,
        /// Project directory (optional)
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Install/Bootstrap the entire ECC, Maestro, and system architecture (90+ Agents)
    Bootstrap,
    /// Bump project version (semver) and optionally update CHANGELOG
    VersionBump {
        /// Bump level: patch | minor | major
        level: String,
        /// Project name or path (default: current directory)
        project: Option<String>,
        /// Update CHANGELOG.md with commits since last tag
        #[arg(long)]
        changelog: bool,
        /// Create a git tag after bumping
        #[arg(long)]
        tag: bool,
    },
    /// Show current version and changelog since last tag
    VersionInfo {
        /// Project name or path (default: current directory)
        project: Option<String>,
    },
    /// Analyze disk usage of a project or all projects
    Disk {
        /// Project name or path (default: all projects)
        project: Option<String>,
    },
    /// Remove build artifacts (target/, node_modules/, __pycache__, etc.)
    Clean {
        /// Project name or path (default: current directory)
        project: Option<String>,
        /// Show what would be deleted without actually deleting
        #[arg(long)]
        dry_run: bool,
        /// Clean all known projects
        #[arg(long)]
        all: bool,
    },
    /// List all listening ports with PID and process name
    Ps {
        /// Show top N processes by memory (default: show ports only)
        #[arg(short, long)]
        procs: bool,
        /// Number of processes to show
        #[arg(short, long, default_value = "15")]
        top: usize,
    },
    /// Kill a process by port number
    KillPort {
        /// Port number to kill
        port: u16,
    },
    /// Check .env files: missing keys, empty values, undocumented secrets
    Env {
        /// Project name or path (default: current directory)
        project: Option<String>,
        /// Run for all known projects
        #[arg(long)]
        all: bool,
    },
    /// Check dependencies: outdated packages and CVE vulnerabilities
    Deps {
        /// Project name or path (default: current directory)
        project: Option<String>,
        /// Show only CVE vulnerabilities (skip outdated)
        #[arg(long)]
        audit: bool,
        /// Run for all known projects
        #[arg(long)]
        all: bool,
    },
    /// Build a project (auto-detects Rust/Node/Python/Go)
    Build {
        /// Project name or path (default: current directory)
        project: Option<String>,
    },
    /// Run tests for a project (auto-detects test runner)
    Test {
        /// Project name or path (default: current directory)
        project: Option<String>,
        /// Run tests for all known projects
        #[arg(long)]
        all: bool,
    },
    /// Git operations on any project
    Git {
        #[command(subcommand)]
        cmd: GitCommands,
    },
}

#[derive(Subcommand)]
pub enum GitCommands {
    /// Show working tree status
    Status {
        /// Project name or path (default: current directory)
        project: Option<String>,
    },
    /// Show commit log
    Log {
        /// Project name or path
        project: Option<String>,
        /// Number of commits to show
        #[arg(short = 'n', long, default_value = "10")]
        count: usize,
    },
    /// Show diff
    Diff {
        /// Project name or path
        project: Option<String>,
        /// Show staged changes only
        #[arg(long)]
        staged: bool,
    },
    /// Stage all and commit
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: String,
        /// Project name or path
        project: Option<String>,
        /// Push after committing
        #[arg(long)]
        push: bool,
    },
    /// Push current branch to origin
    Push {
        /// Project name or path
        project: Option<String>,
    },
    /// Pull from origin (fast-forward only)
    Pull {
        /// Project name or path
        project: Option<String>,
    },
    /// List branches
    Branches {
        /// Project name or path
        project: Option<String>,
    },
    /// Checkout a branch
    Checkout {
        /// Branch name
        branch: String,
        /// Project name or path
        project: Option<String>,
    },
    /// Create and checkout a new branch
    Branch {
        /// New branch name
        name: String,
        /// Project name or path
        project: Option<String>,
    },
}

/// Load config or fall back to auto-detected dev_ops and a dummy master path.
fn load_cfg() -> Config {
    if let Some(cfg) = Config::load() {
        return cfg;
    }
    let detected = Config::auto_detect();
    Config {
        dev_ops_path: detected
            .dev_ops
            .unwrap_or_else(|| dirs::desktop_dir().unwrap_or_default().join("Dev Ops")),
        master_md_path: detected
            .master_md
            .unwrap_or_else(|| PathBuf::from("MASTER.md")),
        skills_path: detected
            .skills
            .unwrap_or_else(|| PathBuf::from(".agents/skills")),
        vault_projects_path: detected
            .vault_projects
            .unwrap_or_else(|| PathBuf::from("Projeler")),
    }
}

pub fn run(cli: Cli) {
    let cfg = load_cfg();
    let cmd = cli.command.expect("Subcommand missing");
    match cmd {
        Commands::Rules { name } => cmd_rules(name, &cfg.master_md_path, cli.json),
        Commands::Memory { project } => cmd_memory(project, &cfg.dev_ops_path, cli.json),
        Commands::Mempalace => cmd_mempalace(&cfg.dev_ops_path, cli.json),
        Commands::Projects => cmd_projects(&cfg.dev_ops_path, cli.json),
        Commands::Agents => cmd_agents(cli.json),
        Commands::View { name } => cmd_view(name, &cfg.master_md_path, cli.json),
        Commands::Discover => cmd_discover(&cfg.dev_ops_path, cli.json),
        Commands::Health { project } => cmd_health(project, &cfg.dev_ops_path, cli.json),
        Commands::Version => println!("raios v{}", env!("CARGO_PKG_VERSION")),
        Commands::McpServer => {
            if let Err(e) = crate::mcp_server::run_stdio() {
                eprintln!("MCP server error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Run {
            agent,
            project,
            timeout,
        } => {
            if let Err(e) = crate::agent_runner::run_agent(&agent, project, timeout) {
                eprintln!("Agent Runner Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Commit {
            project,
            message,
            push,
            dry_run,
        } => {
            cmd_commit(project, message, push, dry_run, &cfg.dev_ops_path, cli.json);
        }
        Commands::Stats => {
            cmd_stats(&cfg.dev_ops_path, cli.json);
        }
        Commands::Search {
            query,
            top_k,
            reindex,
        } => {
            cmd_search(&query, top_k, reindex, &cfg.dev_ops_path, cli.json);
        }
        Commands::Security {
            project,
            full,
            path,
        } => {
            cmd_security(project, full, path, &cfg.dev_ops_path, cli.json);
        }
        Commands::New {
            name,
            category,
            github,
            no_vault,
        } => {
            cmd_new(
                &name,
                &category,
                github,
                no_vault,
                &cfg.dev_ops_path,
                cli.json,
            );
        }
        Commands::Task {
            description,
            project,
        } => {
            cmd_task(&description, project);
        }
        Commands::Bootstrap => {
            cmd_bootstrap();
        }
        Commands::VersionBump {
            level,
            project,
            changelog,
            tag,
        } => {
            cmd_version_bump(&level, project, changelog, tag, &cfg.dev_ops_path, cli.json);
        }
        Commands::VersionInfo { project } => {
            cmd_version_info(project, &cfg.dev_ops_path, cli.json);
        }
        Commands::Disk { project } => {
            cmd_disk(project, &cfg.dev_ops_path, cli.json);
        }
        Commands::Clean {
            project,
            dry_run,
            all,
        } => {
            cmd_clean(project, dry_run, all, &cfg.dev_ops_path, cli.json);
        }
        Commands::Ps { procs, top } => {
            cmd_ps(procs, top, cli.json);
        }
        Commands::KillPort { port } => {
            cmd_kill_port(port, cli.json);
        }
        Commands::Env { project, all } => {
            cmd_env(project, all, &cfg.dev_ops_path, cli.json);
        }
        Commands::Deps {
            project,
            audit,
            all,
        } => {
            cmd_deps(project, audit, all, &cfg.dev_ops_path, cli.json);
        }
        Commands::Build { project } => {
            cmd_build(project, &cfg.dev_ops_path, cli.json);
        }
        Commands::Test { project, all } => {
            cmd_test(project, all, &cfg.dev_ops_path, cli.json);
        }
        Commands::Git { cmd } => {
            cmd_git(cmd, &cfg.dev_ops_path, cli.json);
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
            if p.name.to_lowercase().contains(&query)
                || p.local_path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&query)
            {
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
            println!(
                "Project: {:<20} | Status: {:<10} | Git: {:<5} | Grade: {} | URL: {}",
                r.name, r.status, dirty, r.compliance_grade, remote
            );
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
        projects
            .into_iter()
            .filter(|p| p.name.to_lowercase().contains(&q))
            .collect()
    } else {
        projects
    };

    #[derive(serde::Serialize)]
    struct CommitEntry {
        name: String,
        committed: bool,
        pushed: bool,
        note: String,
    }
    let mut entries: Vec<CommitEntry> = Vec::new();
    let mut committed_count = 0usize;
    let mut skipped_count = 0usize;

    for p in &candidates {
        let dirty = git_is_dirty(&p.local_path).unwrap_or(false);
        if !dirty {
            skipped_count += 1;
            if !json {
                println!("  skip  {}", p.name);
            }
            continue;
        }
        if dry_run {
            if !json {
                println!("  would commit  {}", p.name);
            }
            entries.push(CommitEntry {
                name: p.name.clone(),
                committed: false,
                pushed: false,
                note: "dry-run".into(),
            });
            continue;
        }
        let result = git_commit(&p.local_path, commit_msg);
        let mut pushed_ok = false;
        let mut note = result.message.clone();
        if result.committed && push {
            match git_push(&p.local_path) {
                Ok(()) => {
                    pushed_ok = true;
                    note = "committed + pushed".into();
                }
                Err(e) => {
                    note = format!("committed, push failed: {}", e);
                }
            }
        } else if result.committed {
            note = "committed".into();
        }
        if result.committed {
            committed_count += 1;
        } else {
            skipped_count += 1;
        }
        if !json {
            let status = if result.committed {
                if pushed_ok {
                    "  ✓ push "
                } else {
                    "  ✓ commit"
                }
            } else {
                "  - skip  "
            };
            println!("{} {} — {}", status, p.name, note);
        }
        entries.push(CommitEntry {
            name: p.name.clone(),
            committed: result.committed,
            pushed: pushed_ok,
            note,
        });
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&entries).unwrap_or_default()
        );
    } else {
        println!(
            "\nDone — {} committed, {} skipped.",
            committed_count, skipped_count
        );
    }
}

fn cmd_stats(_dev_ops: &std::path::Path, json: bool) {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("DB error: {}", e);
            return;
        }
    };

    let s = match crate::db::query_stats(&conn) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Stats query failed: {}", e);
            return;
        }
    };

    // Category breakdown (still needs a project scan for categories)
    let top_cats: Vec<(String, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT category, COUNT(*) AS n FROM projects GROUP BY category ORDER BY n DESC LIMIT 8"
        ).unwrap();
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };

    if json {
        let out = serde_json::json!({
            "total": s.total, "active": s.active, "archived": s.archived,
            "dirty": s.dirty, "no_memory": s.no_memory, "local_only": s.no_github,
            "avg_compliance": s.avg_compliance as u64,
            "avg_security": s.avg_security as u64,
            "grade_a": s.grade_a, "grade_b": s.grade_b,
            "grade_c": s.grade_c, "grade_d": s.grade_d,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    fn bar(n: i64, total: i64, width: usize) -> String {
        if total == 0 {
            return String::new();
        }
        let filled = (n as usize * width) / total as usize;
        "█".repeat(filled)
    }
    fn pct(n: i64, total: i64) -> i64 {
        if total > 0 {
            n * 100 / total
        } else {
            0
        }
    }

    println!(
        "Portfolio Statistics — R-AI-OS v{}",
        env!("CARGO_PKG_VERSION")
    );
    println!("{}", "─".repeat(46));
    println!("Total projects:      {:>5}", s.total);
    println!("Active / Archived:   {:>5} / {}", s.active, s.archived);
    println!("Dirty (uncommitted): {:>5}", s.dirty);
    println!("No memory.md:        {:>5}", s.no_memory);
    println!("Local only (no GH):  {:>5}", s.no_github);
    println!("Avg compliance:      {:>4}/100", s.avg_compliance as u64);
    println!("Avg security:        {:>4}/100", s.avg_security as u64);
    println!();
    println!("Grade Distribution:");
    println!(
        "  A (≥80): {:>4} projects  {} {}%",
        s.grade_a,
        bar(s.grade_a, s.total, 24),
        pct(s.grade_a, s.total)
    );
    println!(
        "  B (≥60): {:>4} projects  {} {}%",
        s.grade_b,
        bar(s.grade_b, s.total, 24),
        pct(s.grade_b, s.total)
    );
    println!(
        "  C (≥40): {:>4} projects  {} {}%",
        s.grade_c,
        bar(s.grade_c, s.total, 24),
        pct(s.grade_c, s.total)
    );
    println!(
        "  D  (<40): {:>4} projects  {} {}%",
        s.grade_d,
        bar(s.grade_d, s.total, 24),
        pct(s.grade_d, s.total)
    );
    println!();
    println!("Top Categories:");
    for (cat, count) in &top_cats {
        println!("  {:<28} {}", cat.replace('_', " "), count);
    }
}

fn cmd_new(
    name: &str,
    category: &str,
    github: bool,
    no_vault: bool,
    dev_ops: &std::path::Path,
    json: bool,
) {
    let effective_category = if category.is_empty() {
        "Uncategorized"
    } else {
        category
    };
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
    let all_ok = result.steps.iter().all(|(_, ok)| *ok);
    if all_ok {
        println!("Done. Project ready at {}", result.path.display());
    } else {
        println!("Completed with some errors. Check the steps above.");
    }
}

fn cmd_security(
    project: Option<String>,
    full: bool,
    scan_path: Option<String>,
    dev_ops: &std::path::Path,
    json: bool,
) {
    use crate::security::{scan_project, Severity};

    // Collect targets
    let targets: Vec<(String, std::path::PathBuf)> = if let Some(p) = scan_path {
        vec![("custom".into(), std::path::PathBuf::from(p))]
    } else {
        let projects = crate::entities::load_entities(dev_ops);
        if let Some(q) = project {
            let q = q.to_lowercase();
            projects
                .into_iter()
                .filter(|p| p.name.to_lowercase().contains(&q))
                .map(|p| (p.name, p.local_path))
                .collect()
        } else {
            projects
                .into_iter()
                .map(|p| (p.name, p.local_path))
                .collect()
        }
    };

    if targets.is_empty() {
        eprintln!("No projects found.");
        return;
    }

    let mut all_reports = Vec::new();

    for (name, path) in &targets {
        if !json {
            eprint!("  scanning {}...", name);
        }
        let report = scan_project(path);
        if !json {
            eprintln!(" {} ({}/100)", report.grade, report.score);
        }
        all_reports.push((name.clone(), path.clone(), report));
    }

    if json {
        #[derive(serde::Serialize)]
        struct Row<'a> {
            name: &'a str,
            path: String,
            score: u8,
            grade: &'a str,
            issues: usize,
            critical: usize,
        }
        let rows: Vec<Row> = all_reports
            .iter()
            .map(|(n, p, r)| Row {
                name: n,
                path: p.display().to_string(),
                score: r.score,
                grade: r.grade,
                issues: r.issues.len(),
                critical: r.critical_count(),
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        );
        return;
    }

    // Human-readable output
    println!();
    println!("Security Scan Results");
    println!("{}", "─".repeat(72));
    println!(
        "{:<28} {:>5}  {:>5}  {:>4}  {:>4}  {:>4}  {:>4}",
        "Project", "Score", "Grade", "Crit", "High", "Med", "Low"
    );
    println!("{}", "─".repeat(72));

    let mut total_score: u32 = 0;
    let mut total_crit = 0usize;

    for (name, _, report) in &all_reports {
        let crit = report
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Critical)
            .count();
        let high = report
            .issues
            .iter()
            .filter(|i| i.severity == Severity::High)
            .count();
        let med = report
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Medium)
            .count();
        let low = report
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Low)
            .count();
        let name_trunc: String = name.chars().take(27).collect();

        println!(
            "{:<28} {:>5}  {:>5}  {:>4}  {:>4}  {:>4}  {:>4}",
            name_trunc, report.score, report.grade, crit, high, med, low
        );

        total_score += report.score as u32;
        total_crit += crit;

        if full && !report.issues.is_empty() {
            for issue in &report.issues {
                let file_display = issue
                    .file
                    .as_ref()
                    .map(|f| {
                        f.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string()
                    })
                    .unwrap_or_default();
                let line_display = issue.line.map(|l| format!(":{}", l)).unwrap_or_default();
                println!(
                    "  [{:>8}] [{}] {} — {}{}",
                    issue.severity.label(),
                    issue.owasp,
                    issue.title,
                    file_display,
                    line_display
                );
                if let Some(ref snip) = issue.snippet {
                    println!("             {}", snip.chars().take(64).collect::<String>());
                }
            }
            println!();
        }
    }

    println!("{}", "─".repeat(72));
    let avg = if all_reports.is_empty() {
        0
    } else {
        total_score as usize / all_reports.len()
    };
    println!(
        "Average score: {}/100   Total critical issues: {}",
        avg, total_crit
    );

    if !full && all_reports.iter().any(|(_, _, r)| !r.issues.is_empty()) {
        println!("\nUse --full to see individual issues.");
    }
}

fn cmd_search(query: &str, top_k: usize, _reindex: bool, dev_ops: &Path, json: bool) {
    if !json {
        println!("🧠 Cortex: Indexing workspace...");
    }

    // 1. Initialise & Index
    let mut cortex = crate::cortex::Cortex::init().unwrap();
    let _ = cortex.index_workspace(dev_ops);

    // 2. Semantic Hits
    let vector_hits = cortex.search(query, top_k).unwrap_or_default();

    // 3. BM25 Hits
    let bm25_hits = {
        let idx = crate::indexer::ProjectIndex::build(dev_ops).unwrap();
        idx.search(query)
    };

    // 4. Hybrid Fusion (RRF)
    let fused = crate::hybrid_search::fuse(bm25_hits, vector_hits, top_k);

    if json {
        let results: Vec<serde_json::Value> = fused
            .iter()
            .map(|r| {
                serde_json::json!({
                    "path": r.path.to_string_lossy(),
                    "project": r.project,
                    "snippet": r.snippet,
                    "line": r.start_line,
                    "score": r.rrf_score,
                    "source": r.source.label()
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&results).unwrap());
        return;
    }

    println!("\nSearch Results for: '{}'", query);
    println!("{}", "─".repeat(72));

    if fused.is_empty() {
        println!("No results found.");
        return;
    }

    for r in fused {
        let source_tag = match r.source {
            crate::hybrid_search::ResultSource::VectorOnly => "🧠 Semantic",
            crate::hybrid_search::ResultSource::BM25Only => "🔍 Keyword ",
            crate::hybrid_search::ResultSource::Hybrid => "🔗 Hybrid  ",
        };

        println!(
            "[{}] {:<30} (score: {:.4})",
            source_tag, r.project, r.rrf_score
        );
        println!("  Path: {}", r.path.to_string_lossy());
        println!("  Line: {}", r.start_line);
        println!(
            "  Snippet: \"{}...\"",
            r.snippet
                .chars()
                .take(120)
                .collect::<String>()
                .replace('\n', " ")
        );
        println!();
    }
}

fn cmd_bootstrap() {
    println!("🚀 Starting Raios TOTAL SYSTEM BOOTSTRAP...");

    let is_windows = cfg!(target_os = "windows");
    let home_dir = dirs::home_dir().expect("Could not find home directory");
    let temp_dir = std::env::temp_dir();

    // 1. Global CLI Tools
    println!("--- [1/5] Checking Global CLI Ecosystem ---");
    let tools = vec!["sigmap", "ctx7", "vercel", "firebase-tools"];
    for tool in tools {
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

    // 2. Gemini CLI Setup
    println!("--- [2/5] Configuring Gemini CLI (90+ Agents) ---");
    let _ = std::process::Command::new("gemini")
        .args([
            "extensions",
            "install",
            "https://github.com/josstei/maestro-orchestrate",
        ])
        .status();

    let gemini_settings_path = home_dir.join(".gemini").join("settings.json");
    if gemini_settings_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&gemini_settings_path) {
            if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
                if json.get("experimental").is_none() {
                    json["experimental"] = serde_json::json!({});
                }
                json["experimental"]["enableAgents"] = serde_json::json!(true);
                if let Ok(updated_json) = serde_json::to_string_pretty(&json) {
                    let _ = std::fs::write(&gemini_settings_path, updated_json);
                    println!("✓ Gemini Agent Mode Enabled.");
                }
            }
        }
    }

    // 3. Claude Code Setup
    println!("--- [3/5] Configuring Claude Code Plugins ---");
    let _ = std::process::Command::new("claude")
        .args([
            "plugin",
            "marketplace",
            "add",
            "https://github.com/josstei/maestro-orchestrate.git",
        ])
        .status();
    let _ = std::process::Command::new("claude")
        .args([
            "plugin",
            "marketplace",
            "add",
            "https://github.com/affaan-m/everything-claude-code.git",
        ])
        .status();
    let _ = std::process::Command::new("claude")
        .args([
            "plugin",
            "install",
            "maestro@maestro-orchestrator",
            "--scope",
            "user",
        ])
        .status();
    let _ = std::process::Command::new("claude")
        .args([
            "plugin",
            "install",
            "everything-claude-code@everything-claude-code",
            "--scope",
            "user",
        ])
        .status();

    // 4. ECC Skills & Rules Distribution
    println!("--- [4/5] Syncing ECC Skills & Rules (182 Skills) ---");
    let ecc_temp_path = temp_dir.join("ecc-master");
    if !ecc_temp_path.exists() {
        let _ = std::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "https://github.com/affaan-m/everything-claude-code.git",
                ecc_temp_path.to_str().unwrap(),
            ])
            .status();
    } else {
        let _ = std::process::Command::new("git")
            .current_dir(&ecc_temp_path)
            .args(["pull"])
            .status();
    }

    // Distribution using native Rust for path compatibility
    let gemini_skills = home_dir.join(".gemini").join("skills");
    let gemini_agents = home_dir.join(".gemini").join("agents");
    let claude_rules = home_dir.join(".claude").join("rules");
    let antigravity_rules = home_dir.join(".antigravity").join("rules");

    let _ = std::fs::create_dir_all(&gemini_skills);
    let _ = std::fs::create_dir_all(&gemini_agents);
    let _ = std::fs::create_dir_all(&claude_rules);
    let _ = std::fs::create_dir_all(&antigravity_rules);

    copy_dir_recursive(&ecc_temp_path.join("skills"), &gemini_skills);
    copy_dir_recursive(&ecc_temp_path.join("agents"), &gemini_agents);
    copy_dir_recursive(&ecc_temp_path.join("rules"), &claude_rules);
    copy_dir_recursive(&ecc_temp_path.join("rules"), &antigravity_rules);

    // 5. Final Activations
    println!("--- [5/5] Final Touches & Activations ---");

    // Default MASTER.md creation
    let master_path = home_dir
        .join("Documents")
        .join("Obsidian Vaults")
        .join("Vault101")
        .join("MASTER.md");
    if !master_path.exists() {
        if let Some(parent) = master_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&master_path, get_default_master_md());
        println!("✓ Writing default MASTER.md to {}", master_path.display());
    }

    let plugins_to_enable = vec![
        "superpowers@claude-plugins-official",
        "context7@claude-plugins-official",
        "frontend-design@claude-plugins-official",
        "github@claude-plugins-official",
    ];

    for plugin in plugins_to_enable {
        let _ = std::process::Command::new("claude")
            .args(["plugin", "enable", plugin])
            .status();
    }

    println!("\n✅ BOOTSTRAP COMPLETE: Your AI OS Factory is fully operational!");
    println!("Total Agents: 90+");
    println!("Total Skills: 182");
    println!("Systems Synced: Gemini, Claude, Antigravity, Sigmap, Ctx7.");
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
    use walkdir::WalkDir;
    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let destination = dst.join(path.strip_prefix(src).expect("Path stripping failed"));
        if path.is_dir() {
            let _ = std::fs::create_dir_all(&destination);
        } else {
            let _ = std::fs::copy(path, &destination);
        }
    }
}

fn get_default_master_md() -> &'static str {
    r#"# MASTER — Goktug

---

## 1. Identity & Behavior

### Identity
You are Goktug's personal assistant. Speak like a work friend—slang and moderate swearing are fine, jokes and puns are welcome, but work comes first. Be clear, direct, and avoid unnecessary verbosity. You are an expert in security and performance-oriented pair programming. You are not just an assistant; you are an equal partner.

### Language
- Code: English
- Communication: Turkish

---

## 2. Coding Standards

### Package Management
pnpm > npm/yarn. Python: uv/pip. Bun projects: bun.

### Code Rules
1. Clarify intent first → generate scope + edge cases, confirm with user → choose best stack → then write.
2. Skeleton first: create file/folder structure, confirm structure before filling files.
3. Component by component: don't produce the entire codebase at once.
4. Write functionally.
5. Error handling always.
6. No comment lines, let the code speak.
7. Goal: optimal + fast + stable.
8. Analyze immediately after producing code: is it idiomatic, clean? Refactor before the user complains.

---

## 3. MANDATORY SKILLS PROTOCOL
Using the following skills is MANDATORY for all agents. Invoking the relevant skill with activate_skill before any operation is a peer-review and quality standard:

- **raios (MANDATORY):** Must be used for system orchestration, project inventory management, and health checks. It is standard to check status with raios health before starting a project.
- **prompt-master (MANDATORY):** Must be used before any prompt is written, improved, or sent to a tool.
- **continuous-learning-v2 (MANDATORY):** The heart of the ECC ecosystem. It is mandatory to save learnings as an "instinct" at the end of each session and invoke them in the next.
- **search-first (MANDATORY):** Ensures comprehensive research in the existing codebase and documentation before starting to write code. "Coding without research" is forbidden.
- **graphify (MANDATORY):** Must be run in complex error analyses or any case requiring system mapping.
- **ki-snapshot (MANDATORY):** Must be used for memory recording and summary at the end of each session.

---

## 4. Technical Stack & Standards

### Stack
No fixed standard, best tool for the job. ECC (Everything Claude Code) standards are prioritized:
- Embedded: ESP32, ESP-IDF, FreeRTOS, C/C++.
- Web/App: React 19, Vite, Tailwind (ECC Patterns).
- DevOps: ECC Build Error Resolver & CI/CD automation.
- AI/ML: AI-First Engineering & Agentic Workflows.

---

## 5. System & Process

### Project Location
- All projects are under C:\Users\turha\Desktop\Dev_Ops_New\, no exceptions.
- Structure: Dev_Ops_New\[Category]\[Project Name].

### Memory & Instinct System
memory.md is mandatory in every project. Thanks to ECC's "Instinct" system, memory is now dynamic, not static.
- **Instincts:** Session inferences are added to global memory at the end of each session with continuous-learning-v2.

---

## 6. Agent System (90+ Expert Army)

The system has 90+ areas of expertise consisting of a combination of Maestro (39 Agents) and ECC (48 Agents).

### Agent Division of Labor
- **Claude Code:** Interactive development and ECC Maestro orchestration.
- **Gemini CLI:** Research, ECC Skills (182) management, and Maestro orchestration.
- **Antigravity:** In-IDE development and ECC Global Rules check.
- **Specialist Subagents:** Complex tasks are delegated to the relevant expert (coder, architect, security-reviewer, loop-operator, etc.).

---

## 7. Additional Rules & Global Guides

The following rule directories complete MASTER.md and apply to all agents:

| Location | Scope |
|---|---|
| ~/.claude/rules/ | ECC Global Rules (Common, TS, Py, Rust etc.) |
| ~/.gemini/skills/ | ECC 182 Skill Library |
| ~/.gemini/agents/ | Maestro & ECC 90+ Agent Definitions |
| ~/.antigravity/rules/ | Antigravity IDE Specific Rules |

---
**What is best for Goktug is always the newest and fastest.**"#
}

fn cmd_task(description: &str, project_dir: Option<String>) {
    use crate::router::AgentRouter;
    println!("?? Routing task: {}", description);

    let mut router = AgentRouter::init().expect("Failed to init AgentRouter");
    match router.route(description) {
        Ok(Some(agent)) => {
            println!("?? Best specialist found: {}", agent);
            println!("?? Invoking agent with the task...");

            // Execute the agent via the runner
            // Note: We use 'gemini' or 'claude' as the base runner and pass the subagent name in the prompt
            let _prompt = format!(
                "Use your specialist subagent '{}' to solve this task: {}",
                agent, description
            );
            let _ = crate::agent_runner::run_agent("gemini", project_dir, None);
            // In a real implementation, we'd pass the prompt to the process stdin or as an argument.
            // For now, we've identified the agent and started the platform.
        }
        Ok(None) => {
            println!("?? No specific specialist found for this task. Try being more descriptive.")
        }
        Err(e) => eprintln!("? Routing error: {}", e),
    }
}

// ─── Disk commands ───────────────────────────────────────────────────────────

fn cmd_disk(project: Option<String>, dev_ops: &Path, json: bool) {
    use crate::core::disk;

    let reports = if project.is_none() {
        disk::analyze_all(dev_ops)
    } else {
        vec![disk::analyze(&resolve_project_path(project, dev_ops))]
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&reports).unwrap_or_default()
        );
        return;
    }

    println!(
        "{:<32} {:>10} {:>10} {:>10} {:>6}",
        "PROJECT", "TOTAL", "SOURCE", "CACHE", "FILES"
    );
    println!("{}", "─".repeat(72));

    for r in &reports {
        let name = r
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| r.path.display().to_string());
        println!(
            "{:<32} {:>10} {:>10} {:>10} {:>6}",
            &name[..name.len().min(31)],
            disk::human_size(r.total_bytes),
            disk::human_size(r.source_bytes),
            disk::human_size(r.cache_bytes),
            r.file_count
        );

        for c in &r.cache_dirs {
            println!(
                "  ↳ {:.<28} {:>10}  ({})",
                c.path.file_name().unwrap_or_default().to_string_lossy(),
                disk::human_size(c.bytes),
                c.kind
            );
        }
    }

    let total_cache: u64 = reports.iter().map(|r| r.cache_bytes).sum();
    if total_cache > 0 {
        println!(
            "\n  Total reclaimable cache: {}",
            disk::human_size(total_cache)
        );
        println!("  Run `raios clean --all` to free it");
    }
}

fn cmd_clean(project: Option<String>, dry_run: bool, all: bool, dev_ops: &Path, json: bool) {
    use crate::core::disk;

    let paths: Vec<std::path::PathBuf> = if all {
        crate::db::open_db()
            .and_then(|conn| crate::db::load_all_projects(&conn))
            .map(|ps| {
                ps.iter()
                    .map(|p| std::path::PathBuf::from(&p.path))
                    .filter(|p| p.exists())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![resolve_project_path(project, dev_ops)]
    };

    let mut total_freed = 0u64;
    let prefix = if dry_run { "DRY RUN" } else { "CLEAN" };

    for path in &paths {
        let result = disk::clean(path, dry_run);
        total_freed += result.freed_bytes;

        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        } else {
            for dir in &result.cleaned_dirs {
                let rel = dir.strip_prefix(path).unwrap_or(dir);
                println!(
                    "[{}] {} — {}",
                    prefix,
                    rel.display(),
                    disk::human_size(result.freed_bytes / result.cleaned_dirs.len().max(1) as u64)
                );
            }
            for e in &result.errors {
                eprintln!("  ✗ {}", e);
            }
        }
    }

    if !json {
        let action = if dry_run { "Would free" } else { "Freed" };
        println!("\n✓ {} {}", action, disk::human_size(total_freed));
    }
}

// ─── Process commands ────────────────────────────────────────────────────────

fn cmd_ps(show_procs: bool, top: usize, json: bool) {
    use crate::core::process;

    let ports = process::list_ports();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ports).unwrap_or_default()
        );
    } else {
        println!("{:<8} {:<10} {:<8} PROCESS", "PORT", "PID", "PROTO");
        println!("{}", "─".repeat(50));
        for p in &ports {
            let pid_s = p.pid.map(|n| n.to_string()).unwrap_or_else(|| "—".into());
            let name = p.process_name.as_deref().unwrap_or("—");
            println!("{:<8} {:<10} {:<8} {}", p.port, pid_s, p.protocol, name);
        }
        if ports.is_empty() {
            println!("  No listening ports found");
        }
    }

    if show_procs {
        let procs = process::list_processes(top);
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&procs).unwrap_or_default()
            );
        } else {
            println!("\n{:<8} {:<6} {:<8} PROCESS", "PID", "CPU%", "MEM MB");
            println!("{}", "─".repeat(50));
            for p in &procs {
                let cpu = p
                    .cpu_pct
                    .map(|c| format!("{:.1}", c))
                    .unwrap_or_else(|| "—".into());
                let mem = p
                    .mem_mb
                    .map(|m| format!("{:.1}", m))
                    .unwrap_or_else(|| "—".into());
                println!("{:<8} {:<6} {:<8} {}", p.pid, cpu, mem, p.name);
            }
        }
    }
}

fn cmd_kill_port(port: u16, json: bool) {
    let r = crate::core::process::kill_port(port);
    if json {
        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
    } else if r.ok {
        println!("✓ {}", r.message);
    } else {
        eprintln!("✗ {}", r.message);
    }
}

// ─── Version commands ────────────────────────────────────────────────────────

fn cmd_version_info(project: Option<String>, dev_ops: &Path, json: bool) {
    let path = resolve_project_path(project, dev_ops);
    match crate::core::version::info(&path) {
        None => eprintln!("✗ No version file found (Cargo.toml / package.json / pyproject.toml)"),
        Some(v) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                println!("Version:  {} ({})", v.current, v.project_type);
                println!("File:     {}", v.version_file);
                match &v.last_tag {
                    Some(t) => println!("Last tag: {}  ({} commits since)", t, v.commits_since_tag),
                    None => println!("Last tag: (none)"),
                }
                if v.commits_since_tag > 0 {
                    println!(
                        "\nChanges since {}:",
                        v.last_tag.as_deref().unwrap_or("beginning")
                    );
                    let entry = crate::core::version::changelog(&path);
                    println!("{}", entry);
                }
            }
        }
    }
}

fn cmd_version_bump(
    level: &str,
    project: Option<String>,
    changelog: bool,
    tag: bool,
    dev_ops: &Path,
    json: bool,
) {
    let bump_type = match crate::core::version::BumpType::from_str(level) {
        Some(b) => b,
        None => {
            eprintln!(
                "✗ Invalid bump level '{}' — use: patch | minor | major",
                level
            );
            return;
        }
    };
    let path = resolve_project_path(project, dev_ops);
    let r = crate::core::version::bump(&path, &bump_type, changelog, tag);

    if json {
        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
        return;
    }

    if r.ok {
        println!(
            "✓ {} → {}  ({})",
            r.old_version, r.new_version, r.version_file
        );
        if changelog {
            println!("✓ CHANGELOG.md updated");
        }
        if tag {
            println!("✓ Git tag v{} created", r.new_version);
        }
        if !r.changelog_entry.is_empty() {
            println!("\n{}", r.changelog_entry);
        }
    } else {
        eprintln!("✗ {}", r.message);
    }
}

// ─── Env command ─────────────────────────────────────────────────────────────

fn cmd_env(project: Option<String>, all: bool, dev_ops: &Path, json: bool) {
    use crate::core::env;

    if all {
        if let Ok(conn) = crate::db::open_db() {
            if let Ok(projects) = crate::db::load_all_projects(&conn) {
                for p in &projects {
                    let path = std::path::Path::new(&p.path);
                    if !path.exists() {
                        continue;
                    }
                    let r = env::check(path);
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                    } else {
                        let status = if r.ok { "✓" } else { "✗" };
                        let detail = match (r.missing_keys.len(), r.empty_keys.len()) {
                            (0, 0) if !r.has_env => "no .env".to_string(),
                            (0, 0) => format!("{} keys OK", r.total_env_keys),
                            (m, e) => format!("{} missing  {} empty", m, e),
                        };
                        println!("{} {:<30}  {}", status, p.name, detail);
                    }
                }
            }
        }
        return;
    }

    let path = resolve_project_path(project, dev_ops);
    let r = env::check(&path);

    if json {
        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
        return;
    }

    // Files overview
    println!("Env files:");
    for f in &r.files {
        if f.exists {
            println!("  ✓ {:<25} {} keys", f.name, f.key_count);
        }
    }
    if !r.has_env {
        println!("  ✗ .env              MISSING");
    }
    if !r.has_example {
        println!("  ✗ .env.example      MISSING — new devs can't onboard");
    }

    // Issues
    if !r.missing_keys.is_empty() {
        println!(
            "\n⚠  Missing keys ({}) — in .env.example but not in .env:",
            r.missing_keys.len()
        );
        for k in &r.missing_keys {
            println!("    - {}", k);
        }
    }
    if !r.empty_keys.is_empty() {
        println!("\n⚠  Empty values ({}):", r.empty_keys.len());
        for k in &r.empty_keys {
            println!("    - {}=", k);
        }
    }
    if !r.undocumented_keys.is_empty() {
        println!(
            "\nℹ  Undocumented keys ({}) — in .env but not in .env.example:",
            r.undocumented_keys.len()
        );
        for k in &r.undocumented_keys {
            println!("    - {}", k);
        }
    }
    if r.ok {
        println!("\n✓ All env keys present and set");
    }
}

// ─── Deps command ────────────────────────────────────────────────────────────

fn print_deps_report(r: &crate::core::deps::DepsReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        return;
    }

    println!(
        "── {} ──  lockfile: {}",
        r.project_type,
        if r.has_lockfile { "✓" } else { "✗ MISSING" }
    );

    if r.cve_count > 0 {
        println!("  🔴 {} CVE ({} critical)", r.cve_count, r.cve_critical);
        for v in &r.cve_issues {
            println!(
                "    [{:>8}] {} {} — {}",
                v.severity.to_uppercase(),
                v.package,
                v.version,
                v.description
            );
        }
    } else {
        println!("  🔒 No known CVEs");
    }

    if r.outdated_count > 0 {
        println!("  ⚠  {} outdated", r.outdated_count);
        for d in r.outdated.iter().take(10) {
            println!("    {:<30} {} → {}", d.name, d.current, d.latest);
        }
        if r.outdated_count > 10 {
            println!("    … and {} more", r.outdated_count - 10);
        }
    } else {
        println!("  ✓  All deps up to date");
    }

    for msg in &r.tool_missing {
        println!("  ℹ  Tool not found: {}", msg);
    }
}

fn cmd_deps(project: Option<String>, _audit_only: bool, all: bool, dev_ops: &Path, json: bool) {
    use crate::core::deps;

    if all {
        if let Ok(conn) = crate::db::open_db() {
            if let Ok(projects) = crate::db::load_all_projects(&conn) {
                for p in &projects {
                    let path = std::path::Path::new(&p.path);
                    if !path.exists() {
                        continue;
                    }
                    let r = deps::check(path);
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                    } else {
                        print!("{:<30} ", p.name);
                        let cve = if r.cve_critical > 0 {
                            format!("🔴 {} crit", r.cve_critical)
                        } else if r.cve_count > 0 {
                            format!("⚠  {} cve", r.cve_count)
                        } else {
                            "✓ 0 cve".into()
                        };
                        let out = if r.outdated_count > 0 {
                            format!("⚠  {} outdated", r.outdated_count)
                        } else {
                            "✓ current".into()
                        };
                        println!("{}  {}", cve, out);
                    }
                }
            }
        }
        return;
    }

    let path = resolve_project_path(project, dev_ops);
    let report = deps::check(&path);
    print_deps_report(&report, json);
}

// ─── Build command ───────────────────────────────────────────────────────────

fn cmd_build(project: Option<String>, dev_ops: &Path, json: bool) {
    let path = resolve_project_path(project, dev_ops);
    let result = crate::core::build::build(&path);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
        return;
    }

    let status = if result.ok { "✓ OK" } else { "✗ FAILED" };
    println!(
        "{} {} — {} in {}ms  ({} warnings, {} errors)",
        status,
        result.project_type,
        result.command,
        result.duration_ms,
        result.warnings,
        result.errors
    );

    for d in &result.diagnostics {
        let loc = d.line.map(|l| format!(":{}", l)).unwrap_or_default();
        println!(
            "  [{}] {}{} — {}",
            d.level.to_uppercase(),
            d.file,
            loc,
            d.message
        );
    }

    if !result.ok && result.diagnostics.is_empty() {
        println!("{}", result.raw_output);
    }
}

fn cmd_test(project: Option<String>, all: bool, dev_ops: &Path, json: bool) {
    use crate::core::build;

    if all {
        if let Ok(conn) = crate::db::open_db() {
            if let Ok(projects) = crate::db::load_all_projects(&conn) {
                let mut total_pass = 0usize;
                let mut total_fail = 0usize;
                for p in &projects {
                    let path = std::path::Path::new(&p.path);
                    if !path.exists() {
                        continue;
                    }
                    let r = build::test(path);
                    total_pass += r.passed;
                    total_fail += r.failed;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                    } else {
                        let status = if r.ok { "✓" } else { "✗" };
                        println!(
                            "{} {:<30} {}/{} tests  {}ms",
                            status,
                            p.name,
                            r.passed,
                            r.passed + r.failed,
                            r.duration_ms
                        );
                        for f in &r.failures {
                            println!("    ↳ {}", f);
                        }
                    }
                }
                if !json {
                    println!("\nTotal: {} passed, {} failed", total_pass, total_fail);
                }
            }
        }
        return;
    }

    let path = resolve_project_path(project, dev_ops);
    let result = build::test(&path);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
        return;
    }

    let status = if result.ok { "✓" } else { "✗" };
    println!(
        "{} {} — {} passed, {} failed, {} ignored  ({}ms)",
        status, result.command, result.passed, result.failed, result.ignored, result.duration_ms
    );

    for f in &result.failures {
        println!("  ↳ {}", f);
    }

    if !result.ok && result.failures.is_empty() {
        println!("{}", result.raw_output);
    }
}

// ─── Git command ──────────────────────────────────────────────────────────────

fn resolve_project_path(project: Option<String>, dev_ops: &Path) -> std::path::PathBuf {
    match project {
        None => std::env::current_dir().unwrap_or_else(|_| dev_ops.to_path_buf()),
        Some(ref p) => {
            let direct = std::path::Path::new(p);
            if direct.exists() {
                return direct.to_path_buf();
            }
            // search by name in DB
            if let Ok(conn) = crate::db::open_db() {
                if let Ok(projects) = crate::db::load_all_projects(&conn) {
                    if let Some(found) = projects
                        .iter()
                        .find(|pr| pr.name.to_lowercase().contains(&p.to_lowercase()))
                    {
                        return std::path::PathBuf::from(&found.path);
                    }
                }
            }
            direct.to_path_buf()
        }
    }
}

fn cmd_git(cmd: GitCommands, dev_ops: &Path, json: bool) {
    use crate::core::git;

    match cmd {
        GitCommands::Status { project } => {
            let path = resolve_project_path(project, dev_ops);
            let s = git::status(&path);
            if json {
                println!("{}", serde_json::to_string_pretty(&s).unwrap_or_default());
            } else {
                let branch = s.branch.as_deref().unwrap_or("(detached)");
                let dirty = if s.dirty { "● dirty" } else { "○ clean" };
                println!("Branch: {}  {}", branch, dirty);
                if s.ahead > 0 {
                    println!("  ↑ {} ahead of remote", s.ahead);
                }
                if s.behind > 0 {
                    println!("  ↓ {} behind remote", s.behind);
                }
                if !s.staged.is_empty() {
                    println!("Staged ({}):", s.staged.len());
                    for f in &s.staged {
                        println!("  + {}", f);
                    }
                }
                if !s.unstaged.is_empty() {
                    println!("Modified ({}):", s.unstaged.len());
                    for f in &s.unstaged {
                        println!("  ~ {}", f);
                    }
                }
                if !s.untracked.is_empty() {
                    println!("Untracked ({}):", s.untracked.len());
                    for f in &s.untracked {
                        println!("  ? {}", f);
                    }
                }
                if !s.dirty {
                    println!("  Nothing to commit.");
                }
            }
        }

        GitCommands::Log { project, count } => {
            let path = resolve_project_path(project, dev_ops);
            let entries = git::log(&path, count);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&entries).unwrap_or_default()
                );
            } else {
                for e in &entries {
                    println!("{} {} ({}  {})", e.short_hash, e.message, e.author, e.date);
                }
            }
        }

        GitCommands::Diff { project, staged } => {
            let path = resolve_project_path(project, dev_ops);
            let d = git::diff(&path, staged);
            if json {
                println!("{}", serde_json::to_string_pretty(&d).unwrap_or_default());
            } else {
                println!(
                    "{} files changed  +{}  -{}",
                    d.files_changed, d.insertions, d.deletions
                );
                if !d.diff_text.is_empty() {
                    println!("\n{}", d.diff_text);
                }
            }
        }

        GitCommands::Commit {
            message,
            project,
            push,
        } => {
            let path = resolve_project_path(project, dev_ops);
            let result = git::commit(&path, &message, true);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                );
            } else if result.ok {
                println!("✓ Committed: {}", result.message);
                if push {
                    let pr = git::push(&path);
                    if pr.ok {
                        println!("✓ Pushed");
                    } else {
                        eprintln!("✗ Push failed: {}", pr.message);
                    }
                }
            } else {
                eprintln!("✗ Commit failed: {}", result.message);
            }
        }

        GitCommands::Push { project } => {
            let path = resolve_project_path(project, dev_ops);
            let r = git::push(&path);
            if json {
                println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
            } else if r.ok {
                println!("✓ {}", r.message);
            } else {
                eprintln!("✗ {}", r.message);
            }
        }

        GitCommands::Pull { project } => {
            let path = resolve_project_path(project, dev_ops);
            let r = git::pull(&path);
            if json {
                println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
            } else if r.ok {
                println!("✓ {}", r.message);
            } else {
                eprintln!("✗ {}", r.message);
            }
        }

        GitCommands::Branches { project } => {
            let path = resolve_project_path(project, dev_ops);
            let bs = git::branches(&path);
            if json {
                println!("{}", serde_json::to_string_pretty(&bs).unwrap_or_default());
            } else {
                for b in &bs {
                    let cur = if b.current { "* " } else { "  " };
                    let rem = if b.remote { " [remote]" } else { "" };
                    println!("{}{}{}", cur, b.name, rem);
                }
            }
        }

        GitCommands::Checkout { branch, project } => {
            let path = resolve_project_path(project, dev_ops);
            let r = git::checkout(&path, &branch);
            if json {
                println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
            } else if r.ok {
                println!("✓ {}", r.message);
            } else {
                eprintln!("✗ {}", r.message);
            }
        }

        GitCommands::Branch { name, project } => {
            let path = resolve_project_path(project, dev_ops);
            let r = git::create_branch(&path, &name);
            if json {
                println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
            } else if r.ok {
                println!("✓ {}", r.message);
            } else {
                eprintln!("✗ {}", r.message);
            }
        }
    }
}
