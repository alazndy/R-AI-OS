/// Radar Mode — real-time context whispers pushed to connected agents.
///
/// Whispers are short, actionable hints emitted as the codebase changes.
/// Agents receive them over the TCP broadcast channel as JSON lines with
/// `"event": "RadarWhisper"`. They can filter by `kind` and `severity`.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::broadcast;

// ─── Whisper kinds ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhisperKind {
    CompileError,
    SecurityVuln,
    ArchitecturalViolation,
    CodebaseRule,
    LockConflict,
    WorktreeMergeConflict,
    CveDetected,
}

impl WhisperKind {
    pub fn label(&self) -> &'static str {
        match self {
            WhisperKind::CompileError => "compile_error",
            WhisperKind::SecurityVuln => "security_vuln",
            WhisperKind::ArchitecturalViolation => "arch_violation",
            WhisperKind::CodebaseRule => "codebase_rule",
            WhisperKind::LockConflict => "lock_conflict",
            WhisperKind::WorktreeMergeConflict => "merge_conflict",
            WhisperKind::CveDetected => "cve_detected",
        }
    }
}

// ─── Severity ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Error,
    Critical,
}

// ─── Whisper ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Whisper {
    pub kind: WhisperKind,
    pub severity: Severity,
    /// Relative or absolute file path the whisper refers to, if any.
    pub file: Option<PathBuf>,
    /// Line number within `file`, if known.
    pub line: Option<u32>,
    /// Human-readable message for the agent.
    pub message: String,
    /// Optional machine-readable hint the agent can act on directly.
    pub hint: Option<String>,
    /// Project name this whisper belongs to.
    pub project: String,
}

impl Whisper {
    pub fn compile_error(project: &str, file: PathBuf, line: u32, message: &str) -> Self {
        Self {
            kind: WhisperKind::CompileError,
            severity: Severity::Error,
            file: Some(file),
            line: Some(line),
            message: message.to_string(),
            hint: Some("fix the compile error before continuing".to_string()),
            project: project.to_string(),
        }
    }

    pub fn security_vuln(
        project: &str,
        file: Option<PathBuf>,
        message: &str,
        cve: Option<&str>,
    ) -> Self {
        Self {
            kind: WhisperKind::SecurityVuln,
            severity: Severity::Critical,
            file,
            line: None,
            message: message.to_string(),
            hint: cve.map(|c| format!("see https://nvd.nist.gov/vuln/detail/{c}")),
            project: project.to_string(),
        }
    }

    pub fn arch_violation(project: &str, file: PathBuf, message: &str, hint: Option<&str>) -> Self {
        Self {
            kind: WhisperKind::ArchitecturalViolation,
            severity: Severity::Warn,
            file: Some(file),
            line: None,
            message: message.to_string(),
            hint: hint.map(ToOwned::to_owned),
            project: project.to_string(),
        }
    }

    pub fn lock_conflict(project: &str, resource: &str, owner: &str) -> Self {
        Self {
            kind: WhisperKind::LockConflict,
            severity: Severity::Warn,
            file: None,
            line: None,
            message: format!("Resource '{}' is locked by '{}'", resource, owner),
            hint: Some("wait for the lock to release or use higher priority".to_string()),
            project: project.to_string(),
        }
    }

    pub fn codebase_rule(project: &str, rule: &str, detail: &str) -> Self {
        Self {
            kind: WhisperKind::CodebaseRule,
            severity: Severity::Info,
            file: None,
            line: None,
            message: format!("[{}] {}", rule, detail),
            hint: None,
            project: project.to_string(),
        }
    }
}

// ─── Radar channel ────────────────────────────────────────────────────────────

/// Wraps a broadcast::Sender and serialises whispers as JSON lines.
/// Clone cheaply — all clones share the same underlying channel.
#[derive(Clone)]
pub struct RadarChannel {
    tx: broadcast::Sender<String>,
}

impl RadarChannel {
    pub fn new(tx: broadcast::Sender<String>) -> Self {
        Self { tx }
    }

    /// Emit a whisper to all connected agents.
    pub fn emit(&self, whisper: Whisper) {
        let payload = serde_json::json!({
            "event": "RadarWhisper",
            "kind": whisper.kind.label(),
            "severity": whisper.severity,
            "project": whisper.project,
            "file": whisper.file.as_ref().map(|p| p.to_string_lossy().to_string()),
            "line": whisper.line,
            "message": whisper.message,
            "hint": whisper.hint,
        });
        let _ = self.tx.send(payload.to_string());
    }

    /// Emit multiple whispers at once.
    pub fn emit_many(&self, whispers: impl IntoIterator<Item = Whisper>) {
        for w in whispers {
            self.emit(w);
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_error_whisper_serialises() {
        let w = Whisper::compile_error(
            "r-ai-os",
            PathBuf::from("src/main.rs"),
            42,
            "expected `;` found `}`",
        );
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("compile_error"));
        assert!(json.contains("src/main.rs"));
    }

    #[test]
    fn lock_conflict_whisper_has_hint() {
        let w = Whisper::lock_conflict("myproj", "src/auth.rs", "gemini");
        assert!(w.hint.is_some());
        assert_eq!(w.severity, Severity::Warn);
    }

    #[tokio::test]
    async fn radar_channel_emits_to_subscribers() {
        let (tx, mut rx) = broadcast::channel::<String>(16);
        let radar = RadarChannel::new(tx);

        radar.emit(Whisper::codebase_rule("proj", "pnpm", "use pnpm, not npm"));

        let msg = rx.recv().await.unwrap();
        let val: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(val["event"], "RadarWhisper");
        assert_eq!(val["kind"], "codebase_rule");
    }
}
