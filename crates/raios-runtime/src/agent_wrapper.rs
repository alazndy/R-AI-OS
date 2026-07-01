use std::path::PathBuf;

const BLOCK_BEGIN: &str = "# raios-agent-wrapper-begin";
const BLOCK_END: &str = "# raios-agent-wrapper-end";

pub const ALL_AGENTS: &[&str] = &["claude", "codex", "opencode", "agy"];

#[derive(Debug, Clone)]
pub struct WrapperResult {
    pub desc: String,
    pub ok: bool,
    pub skipped: bool,
}

impl WrapperResult {
    fn ok(desc: impl Into<String>) -> Self { Self { desc: desc.into(), ok: true, skipped: false } }
    fn fail(desc: impl Into<String>) -> Self { Self { desc: desc.into(), ok: false, skipped: false } }
    fn skip(desc: impl Into<String>) -> Self { Self { desc: desc.into(), ok: true, skipped: true } }
}

#[derive(Debug, Clone)]
pub struct AgentShimStatus {
    pub agent: String,
    pub installed: bool,
    pub real_found: bool,
    pub rc_file: String,
}

pub fn detect_rc_paths() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let shell = std::env::var("SHELL").unwrap_or_default();
    let mut candidates: Vec<PathBuf> = if shell.contains("zsh") {
        vec![home.join(".zshrc")]
    } else if shell.contains("bash") {
        let bashrc = home.join(".bashrc");
        let profile = home.join(".bash_profile");
        if bashrc.exists() { vec![bashrc] } else { vec![profile] }
    } else {
        // Unknown shell — try both, prefer zsh
        vec![home.join(".zshrc"), home.join(".bashrc")]
    };
    let existing: Vec<_> = candidates.iter().filter(|p| p.exists()).cloned().collect();
    if existing.is_empty() {
        candidates.truncate(1);
        candidates
    } else {
        existing
    }
}

fn make_block(agents: &[&str]) -> String {
    let mut s = String::from("\n");
    s.push_str(BLOCK_BEGIN);
    s.push('\n');
    s.push_str("# R-AI-OS: route agent calls through raios (UMAI shield + session capture)\n");
    s.push_str("# Remove with: raios agent-wrapper remove\n");
    for agent in agents {
        s.push_str(&format!("{}() {{ raios run {} \"$@\"; }}\n", agent, agent));
    }
    s.push_str(BLOCK_END);
    s.push('\n');
    s
}

fn strip_block(content: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut skip = false;
    for line in content.lines() {
        if line.trim_start() == BLOCK_BEGIN { skip = true; continue; }
        if line.trim_start() == BLOCK_END { skip = false; continue; }
        if !skip { out.push(line); }
    }
    let s = out.join("\n");
    s.trim_end().to_string() + "\n"
}

fn remove_agents_from_block(content: &str, agents: &[&str]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_block = false;
    for line in content.lines() {
        if line.trim_start() == BLOCK_BEGIN {
            in_block = true;
            out.push(line.to_string());
            continue;
        }
        if line.trim_start() == BLOCK_END {
            in_block = false;
            out.push(line.to_string());
            continue;
        }
        if in_block {
            let is_target = agents.iter().any(|a| line.starts_with(&format!("{}() {{", a)));
            if !is_target { out.push(line.to_string()); }
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n") + "\n"
}

/// Install shell wrapper functions for the given agents into the shell rc file.
/// Shell functions intercept the agent command in interactive shells and route
/// through `raios run <agent>`, which applies UMAI shield, handoff injection,
/// and session capture. Because `Command::new("agent")` in Rust bypasses shell
/// functions entirely (it goes straight to execve), there is no recursion risk.
pub fn install(agents: &[&str]) -> Vec<WrapperResult> {
    let mut log = Vec::new();
    let rc_files = detect_rc_paths();

    if rc_files.is_empty() {
        log.push(WrapperResult::fail(
            "shell rc file not found — expected ~/.zshrc or ~/.bashrc",
        ));
        return log;
    }

    let block = make_block(agents);

    for rc_path in &rc_files {
        let existing = std::fs::read_to_string(rc_path).unwrap_or_default();
        let new_content = if existing.contains(BLOCK_BEGIN) {
            // Replace existing block (agent list may have changed)
            strip_block(&existing) + &block
        } else {
            existing + &block
        };
        match std::fs::write(rc_path, &new_content) {
            Ok(_) => log.push(WrapperResult::ok(format!(
                "wrapper functions added → {}",
                rc_path.display()
            ))),
            Err(e) => log.push(WrapperResult::fail(format!(
                "{}: {}",
                rc_path.display(),
                e
            ))),
        }
    }

    log.push(WrapperResult::ok(format!("wrapped: {}", agents.join(", "))));
    log.push(WrapperResult::ok(
        "restart terminal or run: source ~/.zshrc",
    ));
    log
}

/// Remove all wrapper functions (or only the given agents) from the shell rc file.
pub fn remove(filter: Option<&[&str]>) -> Vec<WrapperResult> {
    let mut log = Vec::new();

    for rc_path in detect_rc_paths() {
        let existing = match std::fs::read_to_string(&rc_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if !existing.contains(BLOCK_BEGIN) {
            log.push(WrapperResult::skip(format!(
                "not installed in {}",
                rc_path.display()
            )));
            continue;
        }
        let new_content = match filter {
            Some(agents) => remove_agents_from_block(&existing, agents),
            None => strip_block(&existing),
        };
        match std::fs::write(&rc_path, &new_content) {
            Ok(_) => log.push(WrapperResult::ok(format!(
                "removed from {}",
                rc_path.display()
            ))),
            Err(e) => log.push(WrapperResult::fail(format!(
                "{}: {}",
                rc_path.display(),
                e
            ))),
        }
    }
    log
}

/// Return installation status for each known agent.
pub fn status() -> Vec<AgentShimStatus> {
    let rc_files = detect_rc_paths();
    let combined_content = rc_files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n");

    let rc_label = rc_files
        .iter()
        .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    ALL_AGENTS
        .iter()
        .map(|&agent| {
            let installed =
                combined_content.contains(&format!("{}() {{ raios run {}", agent, agent));
            let real_found = std::process::Command::new("which")
                .arg(agent)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            AgentShimStatus {
                agent: agent.to_string(),
                installed,
                real_found,
                rc_file: rc_label.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_block_contains_all_agents() {
        let block = make_block(ALL_AGENTS);
        for agent in ALL_AGENTS {
            assert!(block.contains(&format!("{}() {{", agent)), "missing {}", agent);
        }
        assert!(block.contains(BLOCK_BEGIN));
        assert!(block.contains(BLOCK_END));
    }

    #[test]
    fn strip_block_removes_wrapper_section() {
        let content = "export PATH=~/.cargo/bin:$PATH\n\n# raios-agent-wrapper-begin\nclaude() { raios run claude \"$@\"; }\n# raios-agent-wrapper-end\nalias ls='ls --color'\n";
        let stripped = strip_block(content);
        assert!(!stripped.contains(BLOCK_BEGIN));
        assert!(!stripped.contains("raios run claude"));
        assert!(stripped.contains("export PATH"));
        assert!(stripped.contains("alias ls"));
    }

    #[test]
    fn remove_agents_from_block_leaves_others() {
        let content = "# raios-agent-wrapper-begin\nclaude() { raios run claude \"$@\"; }\ncodex() { raios run codex \"$@\"; }\n# raios-agent-wrapper-end\n";
        let result = remove_agents_from_block(content, &["claude"]);
        assert!(!result.contains("claude()"));
        assert!(result.contains("codex()"));
    }
}
