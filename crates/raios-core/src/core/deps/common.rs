use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdatedDep {
    pub name: String,
    pub current: String,
    pub latest: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CveIssue {
    pub package: String,
    pub version: String,
    pub severity: String,
    pub description: String,
    pub advisory_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepsReport {
    pub project_type: String,
    pub has_lockfile: bool,
    pub outdated: Vec<OutdatedDep>,
    pub outdated_count: usize,
    pub cve_issues: Vec<CveIssue>,
    pub cve_count: usize,
    pub cve_critical: usize,
    pub tool_missing: Vec<String>,
}

impl DepsReport {
    pub fn empty(project_type: &str) -> Self {
        Self {
            project_type: project_type.into(),
            has_lockfile: false,
            outdated: vec![],
            outdated_count: 0,
            cve_issues: vec![],
            cve_count: 0,
            cve_critical: 0,
            tool_missing: vec![],
        }
    }
}

pub fn cvss_to_severity(cvss: &str) -> &'static str {
    // CVSS v3 score ranges
    let score: f64 = cvss.parse().unwrap_or(0.0);
    match score as u8 {
        9..=10 => "critical",
        7..=8 => "high",
        4..=6 => "medium",
        1..=3 => "low",
        _ => "unknown",
    }
}
