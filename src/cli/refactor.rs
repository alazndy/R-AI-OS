use crate::refactor_scan::{scan_project, RefactorSeverity};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct RefactorFileIssue {
    schema_version: u8,
    file: String,
    lines: usize,
    severity: String,
    reasons: Vec<String>,
}

pub(super) fn cmd_refactor(target: Option<String>, dev_ops: &Path, json: bool) {
    let path = super::resolve_project_path(target, dev_ops);
    if !path.exists() {
        eprintln!("Path does not exist: {}", path.display());
        std::process::exit(1);
    }
    let report = scan_project(&path);
    if json {
        let items: Vec<RefactorFileIssue> = report
            .issues
            .iter()
            .map(|i| RefactorFileIssue {
                schema_version: 1,
                file: i.file.to_string_lossy().to_string(),
                lines: i.lines,
                severity: match i.severity {
                    RefactorSeverity::High => "HIGH".to_string(),
                    RefactorSeverity::Medium => "MEDIUM".to_string(),
                    RefactorSeverity::Low => "LOW".to_string(),
                },
                reasons: i.reasons.clone(),
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&items).unwrap_or_default()
        );
    } else {
        if report.issues.is_empty() {
            println!(
                "No refactor issues found. Grade: {} ({}/100)",
                report.grade, report.score
            );
            return;
        }
        println!(
            "Refactor Report — Grade {} ({}/100)",
            report.grade, report.score
        );
        println!("  HIGH: {} | MED: {}", report.high_count, report.medium_count);
        println!();
        for issue in &report.issues {
            println!(
                "  [{:4}] {} — {}",
                issue.severity.label(),
                issue.file.display(),
                issue.reasons.join("; ")
            );
        }
    }
}
