use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Convert an absolute project path to Claude's directory naming convention.
/// `/home/alaz/dev/core/R-AI-OS` → `-home-alaz-dev-core-R-AI-OS`
pub(super) fn claude_project_dir_name(project_path: &str) -> String {
    project_path.replace('/', "-")
}

/// Find the most recently modified JSONL conversation file for a project,
/// optionally requiring it to have been modified at or after `min_mtime`.
pub fn find_latest_conversation(
    project_path: &str,
    min_mtime: Option<SystemTime>,
) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir_name = claude_project_dir_name(project_path);
    let claude_dir = Path::new(&home).join(".claude/projects").join(&dir_name);

    // CCR sessions (CLAUDE_JOB_DIR is set) always write their JSONL to the home-level
    // project dir (~/.claude/projects/-home-alaz/) regardless of the working project.
    // The job ID is the leading path component of CLAUDE_JOB_DIR and matches the
    // filename prefix of the JSONL (e.g. job 9b3cbb27 → 9b3cbb27-<uuid>.jsonl).
    // Prioritize the CCR JSONL over the primary dir — it is the authoritative transcript
    // for the current session and is always more recent than any leftover project JSONL.
    if let Ok(job_dir) = std::env::var("CLAUDE_JOB_DIR") {
        let job_id = Path::new(&job_dir)
            .file_name()?
            .to_string_lossy()
            .into_owned();
        let ccr_dir = Path::new(&home).join(".claude/projects/-home-alaz");
        if ccr_dir != claude_dir {
            // Search for the exact JSONL belonging to this job.
            for entry in std::fs::read_dir(&ccr_dir).ok()?.flatten() {
                let path = entry.path();
                let fname = path.file_name().unwrap_or_default().to_string_lossy();
                if path.extension().map(|e| e == "jsonl").unwrap_or(false)
                    && fname.starts_with(&*job_id)
                {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(mtime) = meta.modified() {
                            if min_mtime.map(|m| mtime >= m).unwrap_or(true) {
                                return Some(path);
                            }
                        }
                    }
                }
            }
            // Job JSONL not found (not yet created or older than min_mtime) — fall through
            // to the primary dir scan below.
        }
    }

    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&claude_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
            if let Ok(meta) = entry.metadata() {
                if let Ok(mtime) = meta.modified() {
                    if let Some(min) = min_mtime {
                        if mtime < min {
                            continue;
                        }
                    }
                    if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                        best = Some((mtime, path));
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Extract user + assistant text content from a Claude Code JSONL file.
/// Returns a compact transcript string suitable for summarization.
pub fn extract_transcript(jsonl_path: &Path) -> String {
    let Ok(content) = std::fs::read_to_string(jsonl_path) else {
        return String::new();
    };

    let mut parts: Vec<String> = Vec::new();

    for line in content.lines() {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        match obj["type"].as_str().unwrap_or("") {
            "user" => {
                let text = extract_content_text(&obj["message"]["content"]);
                if !text.trim().is_empty() {
                    parts.push(format!("User: {}", truncate(&text, 600)));
                }
            }
            "assistant" => {
                let text = extract_content_text(&obj["message"]["content"]);
                if !text.trim().is_empty() {
                    parts.push(format!("Assistant: {}", truncate(&text, 800)));
                }
            }
            _ => {}
        }
    }

    parts.join("\n\n")
}

fn extract_content_text(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        return arr
            .iter()
            .filter(|item| item["type"] == "text")
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ");
    }
    String::new()
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        format!("{}…", chars[..max].iter().collect::<String>())
    }
}

// ─── Per-agent transcript readers ────────────────────────────────────────────

fn started_secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_codex_transcript(path: &Path, since_secs: u64) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for line in content.lines() {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if obj["ts"].as_u64().unwrap_or(0) < since_secs {
            continue;
        }
        if let Some(text) = obj["text"].as_str() {
            if !text.trim().is_empty() {
                parts.push(format!("User: {}", truncate(text, 600)));
            }
        }
    }
    parts.join("\n\n")
}

fn read_agy_transcript(path: &Path, workspace: &str, since_secs: u64) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for line in content.lines() {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if obj["timestamp"].as_u64().unwrap_or(0) / 1000 < since_secs {
            continue;
        }
        if let Some(ws) = obj["workspace"].as_str() {
            if !workspace.is_empty() && ws != workspace {
                continue;
            }
        }
        if let Some(display) = obj["display"].as_str() {
            if !display.trim().is_empty() {
                parts.push(format!("User: {}", truncate(display, 600)));
            }
        }
    }
    parts.join("\n\n")
}

fn read_opencode_transcript(path: &Path, since_secs: u64) -> String {
    // opencode prompt-history has no per-entry timestamps; use file mtime as gate.
    let Ok(meta) = std::fs::metadata(path) else {
        return String::new();
    };
    if let Ok(mtime) = meta.modified() {
        if started_secs(mtime) < since_secs {
            return String::new();
        }
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let mut parts: Vec<String> = content
        .lines()
        .rev()
        .take(60)
        .filter_map(|line| {
            let obj = serde_json::from_str::<serde_json::Value>(line).ok()?;
            let input = obj["input"].as_str()?;
            if input.trim().is_empty() {
                return None;
            }
            Some(format!("User: {}", truncate(input, 600)))
        })
        .collect();
    parts.reverse();
    parts.join("\n\n")
}

pub fn collect_transcript(agent: &str, project_path: &str, session_started: SystemTime) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let since = started_secs(session_started);
    match agent.to_lowercase().as_str() {
        "claude" => find_latest_conversation(project_path, Some(session_started))
            .map(|p| extract_transcript(&p))
            .unwrap_or_default(),
        "codex" => read_codex_transcript(&PathBuf::from(&home).join(".codex/history.jsonl"), since),
        "agy" | "antigravity" => read_agy_transcript(
            &PathBuf::from(&home).join(".gemini/antigravity-cli/history.jsonl"),
            project_path,
            since,
        ),
        "opencode" => read_opencode_transcript(
            &PathBuf::from(&home).join(".local/state/opencode/prompt-history.jsonl"),
            since,
        ),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_reader_is_time_scoped_but_not_project_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"ts\":100,\"text\":\"old decision\"}\n",
                "not json\n",
                "{\"ts\":200,\"text\":\"we decided to use SQLite\"}\n",
                "{\"ts\":201,\"text\":\"prompt from another project\"}\n",
                "{\"ts\":201,\"text\":\"   \"}\n"
            ),
        )
        .unwrap();

        let transcript = read_codex_transcript(&path, 200);
        assert!(transcript.contains("we decided to use SQLite"));
        assert!(transcript.contains("prompt from another project"));
        assert!(!transcript.contains("old decision"));
    }

    #[test]
    fn agy_reader_scopes_entries_to_the_active_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":200000,\"workspace\":\"/project-a\",\"display\":\"keep this\"}\n",
                "{\"timestamp\":201000,\"workspace\":\"/project-b\",\"display\":\"exclude this\"}\n",
                "{\"timestamp\":199000,\"workspace\":\"/project-a\",\"display\":\"too old\"}\n"
            ),
        )
        .unwrap();

        let transcript = read_agy_transcript(&path, "/project-a", 200);
        assert_eq!(transcript, "User: keep this");
    }

    #[test]
    fn opencode_reader_has_no_project_scope_in_prompt_history_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt-history.jsonl");
        std::fs::write(
            &path,
            "{\"input\":\"prompt from another project\"}\n{\"input\":\"current prompt\"}\n",
        )
        .unwrap();

        let transcript = read_opencode_transcript(&path, 0);
        assert!(transcript.contains("prompt from another project"));
        assert!(transcript.contains("current prompt"));
    }

    #[test]
    fn claude_reader_extracts_text_and_ignores_non_text_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversation.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"use pnpm\"},{\"type\":\"image\"}]}}\n",
                "{\"type\":\"assistant\",\"message\":{\"content\":\"acknowledged\"}}\n",
                "{\"type\":\"tool_result\",\"message\":{\"content\":\"ignore\"}}\n"
            ),
        )
        .unwrap();

        assert_eq!(
            extract_transcript(&path),
            "User: use pnpm\n\nAssistant: acknowledged"
        );
    }
}
