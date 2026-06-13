use std::path::Path;

use super::GitCommands;

pub(super) fn cmd_git(cmd: GitCommands, dev_ops: &Path, json: bool) {
    use crate::core::git;

    match cmd {
        GitCommands::Status { project } => {
            let path = super::resolve_project_path(project, dev_ops);
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
            let path = super::resolve_project_path(project, dev_ops);
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
            let path = super::resolve_project_path(project, dev_ops);
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
            let path = super::resolve_project_path(project, dev_ops);
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
            let path = super::resolve_project_path(project, dev_ops);
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
            let path = super::resolve_project_path(project, dev_ops);
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
            let path = super::resolve_project_path(project, dev_ops);
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
            let path = super::resolve_project_path(project, dev_ops);
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
            let path = super::resolve_project_path(project, dev_ops);
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
