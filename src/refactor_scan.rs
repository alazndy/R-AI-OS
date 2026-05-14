use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefactorSeverity {
    High,
    Medium,
    Low,
}

impl RefactorSeverity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::High => "HIGH",
            Self::Medium => "MED",
            Self::Low => "LOW",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorIssue {
    pub file: PathBuf,
    pub lines: usize,
    pub severity: RefactorSeverity,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorReport {
    pub issues: Vec<RefactorIssue>,
    pub score: u8,
    pub grade: String,
    pub high_count: usize,
    pub medium_count: usize,
}

impl RefactorReport {
    pub fn empty() -> Self {
        Self {
            issues: vec![],
            score: 100,
            grade: "A".into(),
            high_count: 0,
            medium_count: 0,
        }
    }
}

// ─── Constants ───────────────────────────────────────────────────────────────

const SOURCE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "kt", "swift", "go", "java", "cpp", "c",
];

const UNWRAP_PATTERNS: &[(&str, &str)] = &[
    ("rs", ".unwrap()"),
    ("rs", ".expect("),
    ("ts", "as any"),
    ("tsx", "as any"),
    ("kt", "!!"),
];

const HIGH_LINE_THRESHOLD: usize = 500;
const MEDIUM_LINE_THRESHOLD: usize = 300;
const UNWRAP_HIGH: usize = 10;
const UNWRAP_MEDIUM: usize = 5;
const MAX_FILES: usize = 200;

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn scan_project(root: &Path) -> RefactorReport {
    let files = collect_source_files(root);
    let mut issues: Vec<RefactorIssue> = files.iter().filter_map(|f| analyze_file(f)).collect();

    let high_count = issues
        .iter()
        .filter(|i| i.severity == RefactorSeverity::High)
        .count();
    let medium_count = issues
        .iter()
        .filter(|i| i.severity == RefactorSeverity::Medium)
        .count();

    let penalty = (high_count * 15 + medium_count * 5).min(100) as u8;
    let score = 100u8.saturating_sub(penalty);
    let grade = score_to_grade(score).to_string();

    issues.sort_by_key(|i| match i.severity {
        RefactorSeverity::High => 0u8,
        RefactorSeverity::Medium => 1,
        RefactorSeverity::Low => 2,
    });

    RefactorReport {
        issues,
        score,
        grade,
        high_count,
        medium_count,
    }
}

// ─── File analysis ───────────────────────────────────────────────────────────

fn analyze_file(path: &Path) -> Option<RefactorIssue> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let content = std::fs::read_to_string(path).ok()?;
    let lines = content.lines().count();

    let mut reasons: Vec<String> = Vec::new();

    if lines >= HIGH_LINE_THRESHOLD {
        reasons.push(format!("{} lines (>{} limit)", lines, HIGH_LINE_THRESHOLD));
    } else if lines >= MEDIUM_LINE_THRESHOLD {
        reasons.push(format!(
            "{} lines (>{} limit)",
            lines, MEDIUM_LINE_THRESHOLD
        ));
    }

    let unwrap_count = count_risky_patterns(&content, ext);
    if unwrap_count >= UNWRAP_HIGH {
        reasons.push(format!("{} risky patterns (.unwrap/!!)", unwrap_count));
    } else if unwrap_count >= UNWRAP_MEDIUM {
        reasons.push(format!("{} risky patterns", unwrap_count));
    }

    let max_depth = estimate_max_nesting(&content);
    if max_depth >= 5 {
        reasons.push(format!("nesting depth ~{}", max_depth));
    }

    if reasons.is_empty() {
        return None;
    }

    Some(RefactorIssue {
        file: path.to_path_buf(),
        lines,
        severity: determine_severity(lines, unwrap_count, max_depth),
        reasons,
    })
}

fn count_risky_patterns(content: &str, ext: &str) -> usize {
    UNWRAP_PATTERNS
        .iter()
        .filter(|(e, _)| *e == ext)
        .map(|(_, pat)| content.matches(pat).count())
        .sum()
}

fn estimate_max_nesting(content: &str) -> usize {
    content
        .lines()
        .map(|line| {
            let leading = line.len().saturating_sub(line.trim_start().len());
            leading / 4
        })
        .max()
        .unwrap_or(0)
}

fn determine_severity(lines: usize, unwrap_count: usize, max_depth: usize) -> RefactorSeverity {
    let is_high = lines >= HIGH_LINE_THRESHOLD || unwrap_count >= UNWRAP_HIGH || max_depth >= 6;

    let is_medium =
        lines >= MEDIUM_LINE_THRESHOLD || unwrap_count >= UNWRAP_MEDIUM || max_depth >= 5;

    if is_high {
        RefactorSeverity::High
    } else if is_medium {
        RefactorSeverity::Medium
    } else {
        RefactorSeverity::Low
    }
}

fn score_to_grade(score: u8) -> &'static str {
    match score {
        80..=100 => "A",
        60..=79 => "B",
        40..=59 => "C",
        _ => "D",
    }
}

// ─── File collection ─────────────────────────────────────────────────────────

fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_recursive(root, 0, &mut files);
    files
}

fn collect_recursive(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > 4 || files.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() >= MAX_FILES {
            break;
        }
        let path = entry.path();
        let name_str = entry.file_name();
        let name = name_str.to_string_lossy();

        if matches!(
            name.as_ref(),
            "." | ".." | "target" | "node_modules" | "dist" | "__pycache__" | ".git"
        ) || name.starts_with('.')
        {
            continue;
        }

        if path.is_dir() {
            collect_recursive(&path, depth + 1, files);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| SOURCE_EXTS.contains(&e))
            .unwrap_or(false)
        {
            files.push(path);
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_boundaries() {
        assert_eq!(score_to_grade(100), "A");
        assert_eq!(score_to_grade(80), "A");
        assert_eq!(score_to_grade(79), "B");
        assert_eq!(score_to_grade(60), "B");
        assert_eq!(score_to_grade(59), "C");
        assert_eq!(score_to_grade(40), "C");
        assert_eq!(score_to_grade(39), "D");
        assert_eq!(score_to_grade(0), "D");
    }

    #[test]
    fn empty_project_perfect_score() {
        let tmp = std::env::temp_dir().join("raios_rf_empty");
        let _ = std::fs::create_dir_all(&tmp);
        let report = scan_project(&tmp);
        assert_eq!(report.score, 100);
        assert_eq!(report.grade, "A");
        assert_eq!(report.high_count, 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn large_file_triggers_high() {
        let tmp = std::env::temp_dir().join("raios_rf_large");
        let _ = std::fs::create_dir_all(&tmp);
        let big: String = (0..600).map(|i| format!("fn foo_{i}() {{}}\n")).collect();
        std::fs::write(tmp.join("big.rs"), big).unwrap();
        let report = scan_project(&tmp);
        assert!(report.high_count >= 1);
        assert!(report.score < 100);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unwrap_count_triggers_issue() {
        let tmp = std::env::temp_dir().join("raios_rf_unwrap");
        let _ = std::fs::create_dir_all(&tmp);
        let src: String = (0..6).map(|_| "let x = foo.unwrap();\n").collect();
        std::fs::write(tmp.join("risky.rs"), src).unwrap();
        let report = scan_project(&tmp);
        assert!(report.high_count + report.medium_count >= 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn severity_high_on_large_file() {
        assert_eq!(determine_severity(600, 0, 0), RefactorSeverity::High);
    }

    #[test]
    fn severity_medium_on_mid_file() {
        assert_eq!(determine_severity(350, 0, 0), RefactorSeverity::Medium);
    }
}
