use clap::{Parser, Subcommand};
use crate::filebrowser::{
    discover_memory_files, find_file_by_name, get_agent_config_files, get_master_rule_files,
    get_mempalace_files, load_file_content,
};

#[derive(Parser)]
#[command(name = "raios", about = "AI OS Terminal Control Center — Rust Edition", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
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
}

pub fn run(cmd: Commands) {
    match cmd {
        Commands::Rules { name } => cmd_rules(name),
        Commands::Memory { project } => cmd_memory(project),
        Commands::Mempalace => cmd_mempalace(),
        Commands::Projects => cmd_projects(),
        Commands::Agents => cmd_agents(),
        Commands::View { name } => cmd_view(name),
    }
}

fn cmd_rules(filter: Option<String>) {
    let files = get_master_rule_files();
    let filter = filter.as_deref().map(str::to_lowercase);

    for entry in &files {
        if let Some(ref f) = filter {
            if !entry.name.to_lowercase().contains(f.as_str()) {
                continue;
            }
        }
        println!("=== {} ===", entry.name);
        println!("{}", load_file_content(&entry.path));
        println!();
    }
}

fn cmd_memory(project: Option<String>) {
    if let Some(query) = project {
        let q = query.to_lowercase();
        for m in discover_memory_files(100) {
            if m.name.to_lowercase().contains(&q) {
                println!("=== {} ===", m.name);
                println!("{}", load_file_content(&m.path));
                return;
            }
        }
        eprintln!("Memory not found for: {}", query);
        std::process::exit(1);
    } else {
        for m in discover_memory_files(5) {
            println!("=== {} ===", m.name);
            println!("{}", load_file_content(&m.path));
            println!();
        }
    }
}

fn cmd_mempalace() {
    let files = get_mempalace_files();
    if let Some(first) = files.first() {
        println!("{}", load_file_content(&first.path));
    }
}

fn cmd_projects() {
    for entry in discover_memory_files(100) {
        let proj = entry.name.trim_end_matches("/memory.md");
        println!("• {:<40} {}", proj, entry.path.display());
    }
}

fn cmd_agents() {
    for entry in get_agent_config_files() {
        let mark = if entry.exists() { "✓" } else { "✗" };
        println!("[{}] {:<30} {}", mark, entry.name, entry.path.display());
    }
}

fn cmd_view(name: String) {
    match find_file_by_name(&name) {
        Some(entry) => println!("{}", load_file_content(&entry.path)),
        None => {
            eprintln!("File not found: {}", name);
            std::process::exit(1);
        }
    }
}
