use crate::refactor_scan::{
    scan_project_with, PartialThresholds, RefactorConfig, RefactorSeverity, RefactorThresholds,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Serialize)]
struct RefactorFileIssue {
    schema_version: u8,
    file: String,
    lines: usize,
    severity: String,
    reasons: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cmd_refactor(
    target: Option<String>,
    dev_ops: &Path,
    json: bool,
    high_lines: usize,
    medium_lines: usize,
    high_unwrap: usize,
    medium_unwrap: usize,
    high_nesting: usize,
    medium_nesting: usize,
    ext_config: Option<String>,
) {
    let path = super::resolve_project_path(target, dev_ops);
    if !path.exists() {
        eprintln!("Path does not exist: {}", path.display());
        std::process::exit(1);
    }

    let per_ext: HashMap<String, PartialThresholds> = ext_config
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let config = RefactorConfig {
        defaults: RefactorThresholds {
            high_lines,
            medium_lines,
            high_unwrap,
            medium_unwrap,
            high_nesting,
            medium_nesting,
        },
        per_ext,
    };

    let report = scan_project_with(&path, &config);
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
        println!(
            "  HIGH: {} | MED: {}",
            report.high_count, report.medium_count
        );
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
