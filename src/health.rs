use std::path::{Path, PathBuf};
use crate::entities::EntityProject;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectHealth {
    pub name: String,
    pub path: PathBuf,
    pub status: String,
    pub git_dirty: Option<bool>,
    pub compliance_score: Option<u8>,
    pub compliance_grade: &'static str,
    pub has_memory: bool,
    pub constitution_issues: Vec<&'static str>,
    pub graphify_done: bool,
    pub graph_report: Option<PathBuf>,
}

const CONSTITUTION_RULES: &[(&str, &str)] = &[
    ("pnpm",          "pnpm over npm/yarn"),
    ("rls",           "RLS (Row Level Security)"),
    ("api_key",       "no client-side API keys"),
    ("prompt-master", "prompt-master skill"),
    ("graphify",      "graphify skill"),
];

pub fn check_project(proj: &EntityProject) -> ProjectHealth {
    let path = &proj.local_path;

    let git_dirty = crate::filebrowser::git_is_dirty(path);
    let has_memory = path.join("memory.md").exists();
    let (compliance_score, compliance_grade) = compute_compliance(path);
    let constitution_issues = check_constitution(path);
    let (graphify_done, graph_report) = check_graphify(path);

    ProjectHealth {
        name: proj.name.clone(),
        path: path.clone(),
        status: proj.status.clone(),
        git_dirty,
        compliance_score,
        compliance_grade,
        has_memory,
        constitution_issues,
        graphify_done,
        graph_report,
    }
}

/// Returns (done, report_path) — checks for graphify output files in project.
pub fn check_graphify(path: &Path) -> (bool, Option<PathBuf>) {
    let candidates = [
        path.join("graph.html"),
        path.join("GRAPH_REPORT.md"),
        path.join("graphify-out").join("graph.html"),
        path.join("graphify-out").join("GRAPH_REPORT.md"),
        path.join(".graphify").join("graph.html"),
    ];
    for p in &candidates {
        if p.exists() {
            return (true, Some(p.clone()));
        }
    }
    (false, None)
}

/// Find the graphify.py script from known locations.
pub fn find_graphify_script(dev_ops: &Path) -> Option<PathBuf> {
    let candidates = [
        dev_ops.join("AI OS").join("graphify").join("graphify.py"),
        dev_ops.join("AI OS").join("graphify").join("main.py"),
        dev_ops.join("AI OS").join("graphify").join("src").join("graphify.py"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn compute_compliance(path: &Path) -> (Option<u8>, &'static str) {
    for name in &["CLAUDE.md", "memory.md", "README.md"] {
        let p = path.join(name);
        if p.exists() {
            if let Ok(content) = std::fs::read_to_string(&p) {
                let report = crate::compliance::check_file(&p, &content);
                return (Some(report.score), report.grade());
            }
        }
    }
    (None, "-")
}

fn check_constitution(path: &Path) -> Vec<&'static str> {
    let candidates = [
        path.join("CLAUDE.md"),
        path.join(".claude").join("CLAUDE.md"),
    ];
    for p in &candidates {
        if p.exists() {
            return scan_rules(p);
        }
    }
    vec!["no local CLAUDE.md"]
}

fn scan_rules(path: &Path) -> Vec<&'static str> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c.to_lowercase(),
        Err(_) => return vec!["cannot read CLAUDE.md"],
    };
    CONSTITUTION_RULES
        .iter()
        .filter(|(kw, _)| !content.contains(kw))
        .map(|(_, desc)| *desc)
        .collect()
}
