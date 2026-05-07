use std::process::Command;
use std::path::Path;
use anyhow::Result;
use crate::daemon::state::ValidationError;

pub fn run_cargo_test(project_path: &Path) -> Result<Vec<ValidationError>> {
    let output = Command::new("cargo")
        .args(["test", "--message-format=json"])
        .current_dir(project_path)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut errors = Vec::new();

    // Note: Parsing cargo test JSON is different from cargo check.
    // It reports test results. For now, let's look for "failed" results.
    
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["event"] == "test" && v["status"] == "failed" {
                let name = v["name"].as_str().unwrap_or("unknown test");
                let stdout_err = v["stdout"].as_str().unwrap_or("");
                
                errors.push(ValidationError {
                    file: "tests".to_string(),
                    message: format!("Test Failed: {} - {}", name, stdout_err),
                    line: None,
                    source: "cargo test".to_string(),
                });
            }
        }
    }

    Ok(errors)
}

pub fn has_tests(project_path: &Path) -> bool {
    project_path.join("tests").exists() || 
    // Very simple check for internal tests
    std::fs::read_dir(project_path.join("src"))
        .map(|entries| {
            entries.filter_map(|e| e.ok()).any(|e| {
                if let Ok(content) = std::fs::read_to_string(e.path()) {
                    content.contains("#[test]")
                } else {
                    false
                }
            })
        })
        .unwrap_or(false)
}
