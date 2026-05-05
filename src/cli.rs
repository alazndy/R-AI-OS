use clap::{Parser, Subcommand};
use std::path::PathBuf;
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
    }
}

pub fn run(cli: Cli) {
    let cfg = load_cfg();
    let cmd = cli.command.unwrap(); // We know it exists
    match cmd {
        Commands::Rules { name }       => cmd_rules(name, cli.json),
        Commands::Memory { project }   => cmd_memory(project, &cfg.dev_ops_path, cli.json),
        Commands::Mempalace            => cmd_mempalace(&cfg.dev_ops_path, cli.json),
        Commands::Projects             => cmd_projects(&cfg.dev_ops_path, cli.json),
        Commands::Agents               => cmd_agents(cli.json),
        Commands::View { name }        => cmd_view(name, cli.json),
        Commands::Discover             => cmd_discover(&cfg.dev_ops_path, cli.json),
        Commands::Health { project }   => cmd_health(project, &cfg.dev_ops_path, cli.json),
    }
}

fn cmd_rules(filter: Option<String>, json: bool) {
    let files = get_master_rule_files();
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

fn cmd_view(name: String, json: bool) {
    match find_file_by_name(&name) {
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
            println!("Project: {:<20} | Status: {:<10} | Git: {:<5} | Grade: {}", r.name, r.status, dirty, r.compliance_grade);
        }
    }
}
