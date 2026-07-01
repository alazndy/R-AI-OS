use crate::daemon::state::ValidationError;
use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

pub fn run_cargo_check(project_path: &Path) -> Result<Vec<ValidationError>> {
    let output = Command::new("cargo")
        .args(["check", "--message-format=json"])
        .current_dir(project_path)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut errors = Vec::new();

    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if v["reason"] == "compiler-message" {
                let message = v["message"].clone();
                let level = message["level"].as_str().unwrap_or("");

                if level == "error" {
                    let text = message["message"].as_str().unwrap_or("").to_string();
                    let spans = message["spans"].as_array();

                    if let Some(spans) = spans {
                        for span in spans {
                            let file = span["file_name"].as_str().unwrap_or("").to_string();
                            let line_num = span["line_start"].as_u64().map(|n| n as usize);

                            errors.push(ValidationError {
                                file,
                                message: text.clone(),
                                line: line_num,
                                source: "cargo check".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(errors)
}
