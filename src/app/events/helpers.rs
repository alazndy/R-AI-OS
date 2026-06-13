use chrono::Local;
use std::io::Write;
use std::path::Path;

pub fn launch_agent(agent: &str, project_path: &Path) -> String {
    if crate::core::process::launch_in_terminal(agent, project_path) {
        format!("{} launched", agent)
    } else {
        format!(
            "Launch error: no supported terminal launcher found for {}",
            agent
        )
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
