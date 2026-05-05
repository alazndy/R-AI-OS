use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Result;

// ─── Task model ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Task {
    pub text: String,
    pub completed: bool,
    pub agent: Option<String>,   // "claude" | "gemini" | "antigravity"
    pub project: Option<String>, // #ProjectName tag
}

impl Task {
    /// Display text — raw text without agent/project tags.
    pub fn display(&self) -> &str {
        &self.text
    }

    /// Agent short label for the badge.
    pub fn agent_label(&self) -> Option<&str> {
        match self.agent.as_deref() {
            Some("claude")      => Some("◆C"),
            Some("gemini")      => Some("◈G"),
            Some("antigravity") => Some("⬡A"),
            _ => None,
        }
    }
}

// ─── Parse / serialize ────────────────────────────────────────────────────────

/// Public wrapper for use from app.rs.
pub fn parse_task_line(line: &str) -> Option<Task> {
    parse_line(line)
}

/// Parse a markdown checkbox line into a Task.
///   - [ ] Fix the login bug @claude #R-AI-OS
///   - [x] Done task @gemini
fn parse_line(line: &str) -> Option<Task> {
    let t = line.trim();
    let completed;
    let rest;

    if let Some(r) = t.strip_prefix("- [ ] ") {
        completed = false;
        rest = r;
    } else if let Some(r) = t.strip_prefix("- [x] ").or_else(|| t.strip_prefix("- [X] ")) {
        completed = true;
        rest = r;
    } else {
        return None;
    }

    let mut agent: Option<String> = None;
    let mut project: Option<String> = None;
    let mut text_parts: Vec<&str> = Vec::new();

    for word in rest.split_whitespace() {
        if let Some(a) = word.strip_prefix('@') {
            let a_lower = a.to_lowercase();
            if matches!(a_lower.as_str(), "claude" | "gemini" | "antigravity" | "ag") {
                agent = Some(if a_lower == "ag" { "antigravity".into() } else { a_lower });
            } else {
                text_parts.push(word); // Unknown @ tag, keep in text
            }
        } else if let Some(p) = word.strip_prefix('#') {
            if !p.is_empty() {
                project = Some(p.to_string());
            }
        } else {
            text_parts.push(word);
        }
    }

    let text = text_parts.join(" ");
    if text.is_empty() {
        return None;
    }

    Some(Task { text, completed, agent, project })
}

fn serialize(task: &Task) -> String {
    let mark = if task.completed { "x" } else { " " };
    let mut s = format!("- [{}] {}", mark, task.text);
    if let Some(ref a) = task.agent {
        s.push_str(&format!(" @{}", a));
    }
    if let Some(ref p) = task.project {
        s.push_str(&format!(" #{}", p));
    }
    s
}

// ─── I/O ─────────────────────────────────────────────────────────────────────

pub fn load_tasks(dev_ops: &Path) -> Result<Vec<Task>> {
    let path = dev_ops.join("tasks.md");
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(path)?;
    let tasks = content.lines().filter_map(parse_line).collect();
    Ok(tasks)
}

pub fn save_tasks(dev_ops: &Path, tasks: &[Task]) -> Result<()> {
    let path = dev_ops.join("tasks.md");
    let mut out = String::from("# Dev Ops Tasks\n\n");
    for task in tasks {
        out.push_str(&serialize(task));
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}

// ─── Agent dispatch ───────────────────────────────────────────────────────────

/// Send a task to an agent:
/// 1. Copies task text to Windows clipboard (clip.exe)
/// 2. Opens the agent in a new terminal, optionally in the project directory
pub fn dispatch_to_agent(
    task: &Task,
    agent: &str,
    project_path: Option<&PathBuf>,
) -> String {
    let task_msg = build_prompt(task, agent);

    // Copy to clipboard via clip.exe
    let clip_ok = copy_to_clipboard(&task_msg);

    // Determine working directory
    let work_dir = project_path
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());

    // Build the agent command
    let agent_cmd = match agent {
        "gemini"      => "gemini",
        "antigravity" => "antigravity",
        _             => "claude",
    };

    let launched = launch_in_terminal(agent_cmd, &work_dir);

    match (clip_ok, launched) {
        (true,  true)  => format!("✓ Task copied to clipboard + {} launched in {}", agent_cmd, short_path(&work_dir)),
        (true,  false) => format!("✓ Task in clipboard — paste when you open {}", agent_cmd),
        (false, true)  => format!("✓ {} launched (clipboard failed)", agent_cmd),
        (false, false) => format!("✗ Dispatch failed — check {} is on PATH", agent_cmd),
    }
}

fn build_prompt(task: &Task, agent: &str) -> String {
    let proj_ctx = task.project.as_deref()
        .map(|p| format!("Project: {}\n", p))
        .unwrap_or_default();

    format!(
        "=== TASK FROM R-AI-OS ===\n{}Agent: {}\nTask: {}\n========================",
        proj_ctx,
        agent,
        task.text
    )
}

fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = match Command::new("clip.exe")
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().is_ok()
}

fn launch_in_terminal(agent_cmd: &str, work_dir: &str) -> bool {
    use std::process::Command;

    // Try Windows Terminal first
    if Command::new("wt")
        .args(["-d", work_dir, "--", agent_cmd])
        .spawn()
        .is_ok()
    {
        return true;
    }

    // Fallback: new cmd window
    let cmd_str = format!("cd /d \"{}\" && {}", work_dir, agent_cmd);
    Command::new("cmd")
        .args(["/c", "start", "cmd", "/k", &cmd_str])
        .spawn()
        .is_ok()
}

fn short_path(path: &str) -> String {
    path.split(['/', '\\']).last().unwrap_or(path).to_string()
}
