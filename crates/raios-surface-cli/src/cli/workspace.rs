use std::path::Path;

use raios_runtime::filebrowser::{
    discover_memory_files, find_file_by_name, get_agent_config_files, get_master_rule_files,
    get_mempalace_files, load_file_content,
};

pub(super) fn cmd_rules(filter: Option<String>, master_md: &Path, json: bool) {
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
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_default()
        );
    }
}

pub(super) fn cmd_memory(
    project: Option<String>,
    query: Option<String>,
    top: usize,
    dev_ops: &Path,
    json: bool,
) {
    if let Some(q) = query {
        cmd_memory_search(&q, top, dev_ops, json);
        return;
    }

    let files = discover_memory_files(dev_ops, 100);
    let mut results = Vec::new();

    if let Some(filter) = project {
        // If filter is an existing directory, read its memory.md directly
        let direct = std::path::PathBuf::from(&filter);
        if direct.is_dir() {
            let mem = direct.join("memory.md");
            if mem.exists() {
                let name = direct
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| filter.clone());
                if json {
                    if let Ok(content) = std::fs::read_to_string(&mem) {
                        let entry = serde_json::json!({ "name": name, "content": content });
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&entry).unwrap_or_default()
                        );
                    }
                } else {
                    println!("=== {} ===", name);
                    println!("{}", load_file_content(&mem));
                }
            } else {
                eprintln!("No memory.md found in {}", direct.display());
            }
            return;
        }
        // Otherwise search by project name substring
        let f = filter.to_lowercase();
        for m in files {
            if m.name.to_lowercase().contains(&f) {
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
        match serde_json::to_string_pretty(&results) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("JSON serialization error: {e}"),
        }
    }
}

fn cmd_memory_search(query: &str, top: usize, dev_ops: &Path, json: bool) {
    use raios_runtime::cortex::{Cortex, MEMORY_PATTERNS};

    let mut cortex = match Cortex::init() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cortex init failed: {e}. Falling back to plain listing.");
            return;
        }
    };

    if cortex.chunk_count() == 0 {
        eprintln!("Cortex index is empty — indexing memory files first…");
        match cortex.index_memory_files(dev_ops) {
            Ok(n) => eprintln!("Indexed {n} memory file(s)."),
            Err(e) => eprintln!("Index error: {e}"),
        }
    }

    let results = match cortex.search_with_filter(query, top, MEMORY_PATTERNS) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Search error: {e}");
            return;
        }
    };

    if results.is_empty() {
        eprintln!("No memory entries found for query. Try: raios cortex index");
        return;
    }

    if json {
        let json_out: Vec<serde_json::Value> = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let project = std::path::Path::new(&r.path)
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                serde_json::json!({
                    "rank": i + 1, "score": r.score, "project": project,
                    "file": r.path, "line": r.start_line, "snippet": r.text.trim()
                })
            })
            .collect();
        match serde_json::to_string_pretty(&json_out) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("JSON serialization error: {e}"),
        }
    } else {
        for (i, r) in results.iter().enumerate() {
            let score_pct = (r.score * 100.0) as u32;
            let filename = std::path::Path::new(&r.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let project = std::path::Path::new(&r.path)
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let snippet = r
                .text
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>();
            println!(
                "[{}] {}%  {} / {}:{}",
                i + 1,
                score_pct,
                project,
                filename,
                r.start_line
            );
            println!("    \"{}\"", snippet);
            println!();
        }
    }
}

pub(super) fn cmd_mempalace(dev_ops: &Path, json: bool) {
    let files = get_mempalace_files(dev_ops);
    if let Some(first) = files.first() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&first).unwrap_or_default()
            );
        } else {
            println!("{}", load_file_content(&first.path));
        }
    }
}

pub(super) fn cmd_projects(dev_ops: &Path, json: bool) {
    let files = discover_memory_files(dev_ops, 100);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&files).unwrap_or_default()
        );
    } else {
        for entry in files {
            let proj = entry.name.trim_end_matches("/memory.md");
            println!("• {:<40} {}", proj, entry.path.display());
        }
    }
}

pub(super) fn cmd_agents(json: bool) {
    let entries = get_agent_config_files();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&entries).unwrap_or_default()
        );
    } else {
        for entry in entries {
            let mark = if entry.exists() { "✓" } else { "✗" };
            println!("[{}] {:<30} {}", mark, entry.name, entry.path.display());
        }
    }
}

pub(super) fn cmd_view(name: String, master_md: &Path, json: bool) {
    match find_file_by_name(&name, master_md) {
        Some(entry) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&entry).unwrap_or_default()
                );
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

pub(super) fn cmd_discover(dev_ops: &Path, json: bool) {
    let projects = raios_core::entities::discover_entities(dev_ops);
    let _ = raios_core::entities::save_entities(dev_ops, projects.clone());
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&projects).unwrap_or_default()
        );
    } else {
        println!("Discovery complete. Found {} projects.", projects.len());
    }
}
