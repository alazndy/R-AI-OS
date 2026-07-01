use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildDiagnostic {
    pub file: String,
    pub line: Option<usize>,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub ok: bool,
    pub project_type: String,
    pub command: String,
    pub duration_ms: u64,
    pub warnings: usize,
    pub errors: usize,
    pub diagnostics: Vec<BuildDiagnostic>,
    pub raw_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub ok: bool,
    pub project_type: String,
    pub command: String,
    pub duration_ms: u64,
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
    pub failures: Vec<String>,
    pub raw_output: String,
}

pub fn failed_result(ptype: &str, cmd: &str, elapsed: Duration, msg: String) -> BuildResult {
    BuildResult {
        ok: false,
        project_type: ptype.into(),
        command: cmd.into(),
        duration_ms: elapsed.as_millis() as u64,
        warnings: 0,
        errors: 1,
        diagnostics: vec![],
        raw_output: msg,
    }
}

pub fn failed_test(ptype: &str, cmd: &str, elapsed: Duration, msg: String) -> TestResult {
    TestResult {
        ok: false,
        project_type: ptype.into(),
        command: cmd.into(),
        duration_ms: elapsed.as_millis() as u64,
        passed: 0,
        failed: 1,
        ignored: 0,
        failures: vec![msg.clone()],
        raw_output: msg,
    }
}

pub fn extract_num(s: &str, keyword: &str) -> Option<usize> {
    let idx = s.find(keyword)?;
    s[..idx].split_whitespace().last()?.parse().ok()
}
