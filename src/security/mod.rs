pub mod audit;
pub mod auth;
pub mod egress;
pub mod license;
pub mod patterns;
pub mod policy;
pub mod quarantine;
pub mod rate_limiter;
pub mod sandbox;
pub mod scanner;
pub mod secret_lease;
pub mod tool_pin;
pub mod umai;
pub mod verify_chain;

pub use auth::SessionTokenManager;
pub use umai::{Umai, UmaiDecision};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use egress::EgressFilter;
pub use license::{scan_licenses, LicenseDep, LicenseReport};
pub use patterns::{scan_file, WATCHED_EXTS};
pub use policy::PolicyConfig;
pub use rate_limiter::{RateLimitConfig, RateLimiter, ToolRateStatus};
pub use sandbox::{is_path_safe, validate_path};
pub use scanner::scan_project;
pub use verify_chain::{record_audit_event, verify_chain};

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Critical => "CRITICAL",
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Low => "LOW",
            Self::Info => "INFO",
        }
    }
    pub fn deduction(&self) -> i32 {
        match self {
            Self::Critical => 25,
            Self::High => 15,
            Self::Medium => 10,
            Self::Low => 5,
            Self::Info => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityIssue {
    pub owasp: &'static str,
    pub title: &'static str,
    pub severity: Severity,
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReport {
    pub score: u8,
    pub grade: &'static str,
    pub issues: Vec<SecurityIssue>,
    pub audit_output: Option<String>,
    pub project_type: ProjectType,
    pub checks_run: usize,
}

impl SecurityReport {
    pub fn grade_from_score(score: u8) -> &'static str {
        match score {
            90..=100 => "A",
            75..=89 => "B",
            50..=74 => "C",
            25..=49 => "D",
            _ => "F",
        }
    }

    pub fn critical_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Critical)
            .count()
    }

    pub fn high_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::High)
            .count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProjectType {
    Rust,
    NodeJs,
    Python,
    Web,
    Mixed,
    Unknown,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn score_color(score: u8) -> &'static str {
    match score {
        90..=100 => "A",
        75..=89 => "B",
        50..=74 => "C",
        25..=49 => "D",
        _ => "F",
    }
}

pub fn severity_emoji(s: &Severity) -> &'static str {
    match s {
        Severity::Critical => "🔴",
        Severity::High => "🟠",
        Severity::Medium => "🟡",
        Severity::Low => "🔵",
        Severity::Info => "⚪",
    }
}
