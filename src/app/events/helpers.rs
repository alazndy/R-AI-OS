use chrono::Local;
use std::io::Write;
use std::path::Path;

pub fn launch_agent(agent: &str, project_path: &Path) -> String {
    let path_str = project_path.to_string_lossy().into_owned();
    // Try Windows Terminal
    if std::process::Command::new("wt")
        .args(["-d", &path_str, "--", agent])
        .spawn()
        .is_ok()
    {
        return format!("{} launched in Windows Terminal", agent);
    }
    // Fallback: new cmd window
    let cmd_str = format!("cd /d \"{}\" && {}", path_str, agent);
    match std::process::Command::new("cmd")
        .args(["/c", "start", "cmd", "/k", &cmd_str])
        .spawn()
    {
        Ok(_) => format!("{} launched", agent),
        Err(e) => format!("Launch error: {}", e),
    }
}

pub fn append_memo(text: &str, dev_ops: &Path) -> String {
    use std::fs::OpenOptions;
    let ts = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let entry = format!("- [{}] {}\n", ts, text);
    let notes_path = dev_ops.join("_session_notes.md");
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&notes_path)
    {
        Ok(mut f) => {
            let _ = f.write_all(entry.as_bytes());
            "Memo saved → _session_notes.md".to_string()
        }
        Err(e) => format!("Memo error: {}", e),
    }
}
