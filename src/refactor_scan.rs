use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

// ─── Thresholds ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RefactorThresholds {
    pub high_lines: usize,
    pub medium_lines: usize,
    pub high_unwrap: usize,
    pub medium_unwrap: usize,
    /// Nesting depth (leading_spaces / indent_size) to trigger HIGH
    pub high_nesting: usize,
    /// Nesting depth to trigger MEDIUM
    pub medium_nesting: usize,
}

impl Default for RefactorThresholds {
    fn default() -> Self {
        Self {
            high_lines: 500,
            medium_lines: 300,
            high_unwrap: 10,
            medium_unwrap: 5,
            high_nesting: 10,
            medium_nesting: 8,
        }
    }
}

/// Per-extension overrides — each field falls back to the global default when absent.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PartialThresholds {
    pub high_lines: Option<usize>,
    pub medium_lines: Option<usize>,
    pub high_unwrap: Option<usize>,
    pub medium_unwrap: Option<usize>,
    pub high_nesting: Option<usize>,
    pub medium_nesting: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct RefactorConfig {
    pub defaults: RefactorThresholds,
    /// Map from file extension (e.g. "rs", "kt") to partial overrides.
    pub per_ext: HashMap<String, PartialThresholds>,
}

impl RefactorConfig {
    pub fn for_ext(&self, ext: &str) -> RefactorThresholds {
        let p = self.per_ext.get(ext);
        RefactorThresholds {
            high_lines: p.and_then(|x| x.high_lines).unwrap_or(self.defaults.high_lines),
            medium_lines: p.and_then(|x| x.medium_lines).unwrap_or(self.defaults.medium_lines),
            high_unwrap: p.and_then(|x| x.high_unwrap).unwrap_or(self.defaults.high_unwrap),
            medium_unwrap: p.and_then(|x| x.medium_unwrap).unwrap_or(self.defaults.medium_unwrap),
            high_nesting: p.and_then(|x| x.high_nesting).unwrap_or(self.defaults.high_nesting),
            medium_nesting: p
                .and_then(|x| x.medium_nesting)
                .unwrap_or(self.defaults.medium_nesting),
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

const MAX_FILES: usize = 200;

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn scan_project(root: &Path) -> RefactorReport {
    scan_project_with(root, &RefactorConfig::default())
}

pub fn scan_project_with(root: &Path, config: &RefactorConfig) -> RefactorReport {
    let files = collect_source_files(root);
    let mut issues: Vec<RefactorIssue> = files
        .iter()
        .filter_map(|f| {
            let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
            let t = config.for_ext(ext);
            analyze_file(f, &t)
        })
        .collect();

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

fn analyze_file(path: &Path, t: &RefactorThresholds) -> Option<RefactorIssue> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let content = std::fs::read_to_string(path).ok()?;
    let lines = content.lines().count();

    let mut reasons: Vec<String> = Vec::new();

    if lines >= t.high_lines {
        reasons.push(format!("{} lines (>{} limit)", lines, t.high_lines));
    } else if lines >= t.medium_lines {
        reasons.push(format!("{} lines (>{} limit)", lines, t.medium_lines));
    }

    let unwrap_count = count_risky_patterns(&content, ext);
    if unwrap_count >= t.high_unwrap {
        reasons.push(format!("{} risky patterns (.unwrap/!!)", unwrap_count));
    } else if unwrap_count >= t.medium_unwrap {
        reasons.push(format!("{} risky patterns", unwrap_count));
    }

    let max_depth = estimate_max_nesting(&content);
    if max_depth >= t.medium_nesting {
        reasons.push(format!("nesting depth ~{}", max_depth));
    }

    if reasons.is_empty() {
        return None;
    }

    Some(RefactorIssue {
        file: path.to_path_buf(),
        lines,
        severity: determine_severity(lines, unwrap_count, max_depth, t),
        reasons,
    })
}

fn count_risky_patterns(content: &str, ext: &str) -> usize {
    let mut count = 0;
    let patterns: Vec<&str> = UNWRAP_PATTERNS
        .iter()
        .filter(|(e, _)| *e == ext)
        .map(|(_, pat)| *pat)
        .collect();

    if patterns.is_empty() {
        return 0;
    }

    for line in content.lines() {
        let trimmed = line.trim();
        
        // Skip Rust test modules since unwraps are standard practice there
        if ext == "rs" && (trimmed.contains("mod tests") || trimmed.contains("#[cfg(test)]")) {
            break; // Test modules are typically at the end of the file
        }

        // Skip individual test cases and assertions
        if ext == "rs" && (trimmed.contains("#[test]") || trimmed.contains("assert!")) {
            continue;
        }

        for pat in &patterns {
            count += trimmed.matches(pat).count();
        }
    }
    count
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

fn determine_severity(
    lines: usize,
    unwrap_count: usize,
    max_depth: usize,
    t: &RefactorThresholds,
) -> RefactorSeverity {
    let is_high =
        lines >= t.high_lines || unwrap_count >= t.high_unwrap || max_depth >= t.high_nesting;
    let is_medium = lines >= t.medium_lines
        || unwrap_count >= t.medium_unwrap
        || max_depth >= t.medium_nesting;

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
        let t = RefactorThresholds::default();
        assert_eq!(determine_severity(600, 0, 0, &t), RefactorSeverity::High);
    }

    #[test]
    fn severity_medium_on_mid_file() {
        let t = RefactorThresholds::default();
        assert_eq!(determine_severity(350, 0, 0, &t), RefactorSeverity::Medium);
    }

    #[test]
    fn nesting_depth_6_no_longer_triggers_high() {
        let t = RefactorThresholds::default();
        assert_eq!(determine_severity(0, 0, 6, &t), RefactorSeverity::Low);
    }

    #[test]
    fn nesting_depth_10_triggers_high() {
        let t = RefactorThresholds::default();
        assert_eq!(determine_severity(0, 0, 10, &t), RefactorSeverity::High);
    }

    #[test]
    fn nesting_depth_8_triggers_medium() {
        let t = RefactorThresholds::default();
        assert_eq!(determine_severity(0, 0, 8, &t), RefactorSeverity::Medium);
    }

    #[test]
    fn custom_thresholds_change_severity() {
        let t = RefactorThresholds {
            high_lines: 1000,
            medium_lines: 700,
            ..Default::default()
        };
        assert_eq!(determine_severity(600, 0, 0, &t), RefactorSeverity::Low);
    }
}
