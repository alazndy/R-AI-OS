use super::common::{DepsReport, OutdatedDep};
use std::path::Path;
use std::process::Command;

pub fn check_go(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Go");
    report.has_lockfile = dir.join("go.sum").exists();

    // go list -u -m -json all
    let out = Command::new("go")
        .args(["list", "-u", "-m", "-json", "all"])
        .current_dir(dir)
        .output();

    if let Ok(o) = out {
        let stdout = String::from_utf8_lossy(&o.stdout);
        // go list outputs multiple JSON objects concatenated — parse each
        let mut depth = 0i32;
        let mut buf = String::new();
        for ch in stdout.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    buf.push(ch);
                }
                '}' => {
                    depth -= 1;
                    buf.push(ch);
                    if depth == 0 {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&buf) {
                            if let Some(update) = v.get("Update") {
                                let name = v["Path"].as_str().unwrap_or("?").to_string();
                                let current = v["Version"].as_str().unwrap_or("?").to_string();
                                let latest = update["Version"].as_str().unwrap_or("?").to_string();
                                report.outdated.push(OutdatedDep {
                                    name,
                                    current,
                                    latest,
                                    kind: "module".into(),
                                });
                            }
                        }
                        buf.clear();
                    }
                }
                _ if depth > 0 => buf.push(ch),
                _ => {}
            }
        }
        report.outdated_count = report.outdated.len();
    }

    // govulncheck (optional)
    let vuln = Command::new("govulncheck")
        .args(["./..."])
        .current_dir(dir)
        .output();

    if vuln.is_err() {
        report.tool_missing.push(
            "govulncheck (install: go install golang.org/x/vuln/cmd/govulncheck@latest)".into(),
        );
    }

    report
}
