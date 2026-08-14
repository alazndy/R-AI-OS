use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

// ─── Task model ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    /// `Some(uuid)` — bound to a canonical cp_tasks row.
    /// `None`       — not yet persisted (new draft, markdown fallback, or
    ///                created before DB sync e.g. from commands.rs literals).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub text: String,
    pub completed: bool,
    pub agent: Option<String>,   // "claude" | "antigravity" | "opencode"
    pub project: Option<String>, // #ProjectName tag
}

impl Task {
    /// Display text — raw text without agent/project tags.
    pub fn display(&self) -> &str {
        &self.text
    }

    /// Returns true when this task has a canonical DB identity.
    pub fn is_persisted(&self) -> bool {
        self.id.is_some()
    }

    /// Construct a Task from a canonical DB row.
    pub fn from_personal_row(r: raios_core::db::PersonalTaskRow) -> Self {
        Self {
            id: Some(r.id),
            text: r.title,
            completed: r.completed,
            agent: r.assignee_id,
            project: r.project_name,
        }
    }

    /// Agent short label for the badge.
    pub fn agent_label(&self) -> Option<&str> {
        match self.agent.as_deref() {
            Some("claude") => Some("◆C"),
            Some("codex") => Some("⬣X"),
            Some("opencode") => Some("◈O"),
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
///   - [x] Done task
fn parse_line(line: &str) -> Option<Task> {
    let t = line.trim();
    let completed;
    let rest;

    if let Some(r) = t.strip_prefix("- [ ] ") {
        completed = false;
        rest = r;
    } else if let Some(r) = t
        .strip_prefix("- [x] ")
        .or_else(|| t.strip_prefix("- [X] "))
    {
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
            if matches!(
                a_lower.as_str(),
                "claude" | "antigravity" | "codex" | "cx" | "opencode"
            ) {
                agent = Some(match a_lower.as_str() {
                    "cx" => "codex".into(),
                    _ => a_lower,
                });
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

    Some(Task {
        id: None,
        text,
        completed,
        agent,
        project,
    })
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

fn load_from_markdown(dev_ops: &Path) -> Result<Vec<Task>> {
    let path = dev_ops.join("tasks.md");
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(path)?;
    Ok(content.lines().filter_map(parse_line).collect())
}

fn write_markdown(dev_ops: &Path, tasks: &[Task]) -> Result<()> {
    let path = dev_ops.join("tasks.md");
    let mut out = String::from("# Dev Ops Tasks\n\n");
    for task in tasks {
        out.push_str(&serialize(task));
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}

pub fn load_tasks(dev_ops: &Path) -> Result<Vec<Task>> {
    if let Ok(conn) = raios_core::db::open_db() {
        let rows = raios_core::db::cp_list_personal_tasks(&conn).unwrap_or_default();
        if !rows.is_empty() {
            return Ok(rows.into_iter().map(Task::from_personal_row).collect());
        }
        // cp_tasks is empty — migrate from tasks.md if it exists
        let markdown_tasks = load_from_markdown(dev_ops)?;
        if !markdown_tasks.is_empty() {
            let source_path = dev_ops.join("tasks.md").to_string_lossy().into_owned();
            let inputs: Vec<_> = markdown_tasks
                .iter()
                .enumerate()
                .map(|(i, t)| raios_core::db::PersonalTaskInput {
                    id: None,
                    title: t.text.clone(),
                    completed: t.completed,
                    agent: t.agent.clone(),
                    project_name: t.project.clone(),
                    display_order: i as i64,
                })
                .collect();
            let _ = raios_core::db::cp_sync_personal_tasks(&conn, &inputs, &source_path);
            let rows = raios_core::db::cp_list_personal_tasks(&conn).unwrap_or_default();
            return Ok(rows.into_iter().map(Task::from_personal_row).collect());
        }
        return Ok(vec![]);
    }
    // DB unavailable — fall back to markdown (tasks will have id: None)
    load_from_markdown(dev_ops)
}

pub fn save_tasks(dev_ops: &Path, tasks: &[Task]) -> Result<()> {
    if let Ok(conn) = raios_core::db::open_db() {
        let source_path = dev_ops.join("tasks.md").to_string_lossy().into_owned();
        let inputs: Vec<_> = tasks
            .iter()
            .enumerate()
            .map(|(i, t)| raios_core::db::PersonalTaskInput {
                id: t.id.clone(),
                title: t.text.clone(),
                completed: t.completed,
                agent: t.agent.clone(),
                project_name: t.project.clone(),
                display_order: i as i64,
            })
            .collect();
        if raios_core::db::cp_sync_personal_tasks(&conn, &inputs, &source_path).is_ok() {
            let _ = raios_core::db::cp_rebuild_personal_markdown(&conn, dev_ops);
            return Ok(());
        }
    }
    // DB unavailable or sync failed — fall back to direct markdown write
    write_markdown(dev_ops, tasks)
}

// ─── Agent dispatch ───────────────────────────────────────────────────────────

/// Send a task to an agent:
/// 1. Copies task text to the host clipboard
/// 2. Opens the agent in a new terminal, optionally in the project directory
pub fn dispatch_to_agent(
    task: &Task,
    agent: &str,
    project_path: Option<&PathBuf>,
    sentinel_errors: Option<Vec<String>>,
) -> String {
    let task_msg = build_prompt(task, agent, sentinel_errors);

    // Copy to clipboard using the host OS clipboard tool
    let clip_ok = copy_to_clipboard(&task_msg);

    // Determine working directory
    let work_dir = project_path
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());

    // Build the agent command
    let agent_cmd = match agent {
        "codex" => "codex",
        "opencode" => "opencode",
        // identity name → installed CLI binary
        "antigravity" | "agy" => "agy",
        _ => "claude",
    };

    let launched = launch_in_terminal(agent_cmd, &work_dir);

    match (clip_ok, launched) {
        (true, true) => format!(
            "✓ Task copied to clipboard + {} launched in {}",
            agent_cmd,
            short_path(&work_dir)
        ),
        (true, false) => format!("✓ Task in clipboard — paste when you open {}", agent_cmd),
        (false, true) => format!("✓ {} launched (clipboard failed)", agent_cmd),
        (false, false) => format!("✗ Dispatch failed — check {} is on PATH", agent_cmd),
    }
}

fn build_prompt(task: &Task, agent: &str, sentinel_errors: Option<Vec<String>>) -> String {
    let proj_ctx = task
        .project
        .as_deref()
        .map(|p| format!("Project: {}\n", p))
        .unwrap_or_default();

    let mut msg = format!(
        "=== TASK FROM R-AI-OS ===\n{}Agent: {}\nTask: {}\n",
        proj_ctx, agent, task.text
    );

    if let Some(errors) = sentinel_errors {
        if !errors.is_empty() {
            msg.push_str("\n⚠️ SENTINEL ALERT: The following errors were detected in your project. PLEASE FIX THEM:\n");
            for err in errors {
                msg.push_str(&format!("  - {}\n", err));
            }
        }
    }

    msg.push_str("========================");
    msg
}

fn copy_to_clipboard(text: &str) -> bool {
    raios_core::core::process::copy_to_clipboard(text)
}

fn launch_in_terminal(agent_cmd: &str, work_dir: &str) -> bool {
    raios_core::core::process::launch_in_terminal(agent_cmd, std::path::Path::new(work_dir))
}

fn short_path(path: &str) -> String {
    path.split(['/', '\\'])
        .next_back()
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_checkbox_with_tags() {
        let t = parse_task_line("- [ ] Fix the login bug @claude #R-AI-OS").unwrap();
        assert_eq!(t.text, "Fix the login bug");
        assert!(!t.completed);
        assert_eq!(t.agent.as_deref(), Some("claude"));
        assert_eq!(t.project.as_deref(), Some("R-AI-OS"));
        assert!(t.id.is_none());
    }

    #[test]
    fn parses_done_checkbox_case_insensitive() {
        let lower = parse_task_line("- [x] Ship it").unwrap();
        let upper = parse_task_line("- [X] Ship it").unwrap();
        assert!(lower.completed);
        assert!(upper.completed);
        assert_eq!(lower.text, "Ship it");
        assert_eq!(upper.text, "Ship it");
    }

    #[test]
    fn normalizes_cx_agent_alias_to_codex() {
        let t = parse_task_line("- [ ] Run scan @cx").unwrap();
        assert_eq!(t.agent.as_deref(), Some("codex"));
    }

    #[test]
    fn unknown_at_tag_stays_in_text() {
        let t = parse_task_line("- [ ] Fix bug @mystery-agent").unwrap();
        assert_eq!(t.agent, None);
        assert_eq!(t.text, "Fix bug @mystery-agent");
    }

    #[test]
    fn unknown_agent_case_is_normalized_to_lowercase() {
        let t = parse_task_line("- [ ] Fix bug @Claude").unwrap();
        assert_eq!(t.agent.as_deref(), Some("claude"));
    }

    #[test]
    fn ignores_empty_project_tag() {
        let t = parse_task_line("- [ ] Fix bug #").unwrap();
        assert_eq!(t.project, None);
        assert_eq!(t.text, "Fix bug");
    }

    #[test]
    fn empty_text_returns_none() {
        assert!(parse_task_line("- [ ] @claude").is_none());
        assert!(parse_task_line("- [ ]").is_none());
        assert!(parse_task_line("").is_none());
    }

    #[test]
    fn non_checkbox_lines_return_none() {
        assert!(parse_task_line("* not a task").is_none());
        assert!(parse_task_line("- plain list item").is_none());
        assert!(parse_task_line("  [ ] not a task").is_none());
    }

    #[test]
    fn serialize_round_trips_through_parse() {
        let task = Task {
            id: None,
            text: "Write tests".into(),
            completed: true,
            agent: Some("opencode".into()),
            project: Some("raios".into()),
        };
        let line = serialize(&task);
        assert_eq!(line, "- [x] Write tests @opencode #raios");
        let reparsed = parse_task_line(&line).unwrap();
        assert_eq!(reparsed.text, task.text);
        assert_eq!(reparsed.completed, task.completed);
        assert_eq!(reparsed.agent, task.agent);
        assert_eq!(reparsed.project, task.project);
    }

    #[test]
    fn agent_label_maps_known_harnesses() {
        let claude = Task {
            id: None,
            text: "".into(),
            completed: false,
            agent: Some("claude".into()),
            project: None,
        };
        let codex = Task {
            id: None,
            text: "".into(),
            completed: false,
            agent: Some("codex".into()),
            project: None,
        };
        let opencode = Task {
            id: None,
            text: "".into(),
            completed: false,
            agent: Some("opencode".into()),
            project: None,
        };
        let antigravity = Task {
            id: None,
            text: "".into(),
            completed: false,
            agent: Some("antigravity".into()),
            project: None,
        };
        let unknown = Task {
            id: None,
            text: "".into(),
            completed: false,
            agent: Some("other".into()),
            project: None,
        };
        let none = Task {
            id: None,
            text: "".into(),
            completed: false,
            agent: None,
            project: None,
        };

        assert_eq!(claude.agent_label(), Some("◆C"));
        assert_eq!(codex.agent_label(), Some("⬣X"));
        assert_eq!(opencode.agent_label(), Some("◈O"));
        assert_eq!(antigravity.agent_label(), Some("⬡A"));
        assert_eq!(unknown.agent_label(), None);
        assert_eq!(none.agent_label(), None);
    }

    #[test]
    fn is_persisted_tracks_db_identity() {
        let persisted = Task {
            id: Some("abc".into()),
            text: "".into(),
            completed: false,
            agent: None,
            project: None,
        };
        let draft = Task {
            id: None,
            text: "".into(),
            completed: false,
            agent: None,
            project: None,
        };
        assert!(persisted.is_persisted());
        assert!(!draft.is_persisted());
        assert_eq!(draft.display(), "");
    }

    #[test]
    fn from_personal_row_maps_db_fields() {
        let row = raios_core::db::PersonalTaskRow {
            id: "row-1".into(),
            title: "Task title".into(),
            completed: true,
            assignee_id: Some("claude".into()),
            project_name: Some("proj".into()),
            display_order: 3,
        };
        let task = Task::from_personal_row(row);
        assert_eq!(task.id.as_deref(), Some("row-1"));
        assert_eq!(task.text, "Task title");
        assert!(task.completed);
        assert_eq!(task.agent.as_deref(), Some("claude"));
        assert_eq!(task.project.as_deref(), Some("proj"));
    }

    #[test]
    fn build_prompt_includes_project_context() {
        let task = Task {
            id: None,
            text: "Fix it".into(),
            completed: false,
            agent: None,
            project: Some("acme".into()),
        };
        let prompt = build_prompt(&task, "claude", None);
        assert!(prompt.contains("Project: acme"));
        assert!(prompt.contains("Agent: claude"));
        assert!(prompt.contains("Task: Fix it"));
        assert!(!prompt.contains("SENTINEL ALERT"));
    }

    #[test]
    fn build_prompt_appends_sentinel_errors() {
        let task = Task {
            id: None,
            text: "Fix it".into(),
            completed: false,
            agent: None,
            project: None,
        };
        let prompt = build_prompt(&task, "codex", Some(vec!["src/a.ts:3: console.log".into()]));
        assert!(prompt.contains("SENTINEL ALERT"));
        assert!(prompt.contains("src/a.ts:3: console.log"));
    }

    #[test]
    fn build_prompt_ignores_empty_sentinel_list() {
        let task = Task {
            id: None,
            text: "Fix it".into(),
            completed: false,
            agent: None,
            project: None,
        };
        let prompt = build_prompt(&task, "codex", Some(vec![]));
        assert!(!prompt.contains("SENTINEL ALERT"));
    }

    #[test]
    fn short_path_keeps_last_component_on_both_separators() {
        assert_eq!(short_path("/home/user/dev/proj"), "proj");
        assert_eq!(short_path("C:\\Users\\dev\\proj"), "proj");
        assert_eq!(short_path("no-separator"), "no-separator");
    }
}
