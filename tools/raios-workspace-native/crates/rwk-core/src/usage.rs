//! The `raios usage` quota signal, read through the CLI.
//!
//! The figures are local logs and auth metadata the RAIOS CLI reads (`raios usage --json`).
//! This module owns the contract with that CLI: one child process per reading, a bounded wait,
//! and JSON parsing into typed rows. The status bar renders [`UsageReport::quota_line`], and
//! nothing blocks on a reading — the workbench reads it on the background executor.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, ErrorCode, Result};
use crate::search::find_in_path;

/// How long `raios usage` may run before the child is killed.
///
/// The CLI answers from local logs and auth metadata and returns quickly; the budget bounds a
/// hang without ever holding the caller.
pub const USAGE_TIMEOUT: Duration = Duration::from_secs(10);

/// One provider row of `raios usage --json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UsageProvider {
    /// The provider's display name, e.g. `Claude Code`.
    pub provider: String,
    /// The CLI found the provider installed.
    #[serde(default)]
    pub installed: bool,
    /// The provider answered as authenticated.
    #[serde(default)]
    pub authenticated: bool,
    /// The remaining-quota summary, e.g. `5h:64% 7d:8% remaining`; `null` when unknown.
    #[serde(default)]
    pub remaining: Option<String>,
}

/// What `raios usage --json` answered, in the CLI's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageReport {
    /// One row per provider the CLI knows about.
    pub providers: Vec<UsageProvider>,
}

impl UsageReport {
    /// The one-line quota summary for the status bar: every provider with a remaining figure,
    /// as `Kota: Claude Code 5h:64% 7d:8% remaining`. `None` when no provider has one.
    pub fn quota_line(&self) -> Option<String> {
        let rows: Vec<String> = self
            .providers
            .iter()
            .filter_map(|provider| {
                let remaining = provider.remaining.as_deref()?.trim();
                (!remaining.is_empty()).then(|| format!("{} {remaining}", provider.provider.trim()))
            })
            .collect();
        (!rows.is_empty()).then(|| format!("Kota: {}", rows.join(" · ")))
    }
}

/// One quota reading; the workbench and tests can script answers behind this.
pub trait UsageSource: Send + Sync {
    /// Reads the current quota signal.
    fn usage(&self) -> Result<UsageReport>;
}

/// The production source: runs the `raios` CLI.
#[derive(Debug, Clone, Default)]
pub struct CliUsage {
    program: Option<PathBuf>,
}

impl CliUsage {
    /// A source that runs `program`; `None` resolves the first `raios` on `PATH` per call.
    pub fn new(program: Option<PathBuf>) -> Self {
        Self { program }
    }
}

impl UsageSource for CliUsage {
    fn usage(&self) -> Result<UsageReport> {
        match &self.program {
            Some(program) => usage_with(program),
            None => usage(),
        }
    }
}

/// Reads the quota signal with the first `raios` on `PATH`.
pub fn usage() -> Result<UsageReport> {
    let program = find_in_path(std::ffi::OsStr::new("raios")).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Kota sinyali için Raios CLI gerekli; `raios` PATH üzerinde bulunamadı.",
        )
    })?;
    usage_with(&program)
}

/// Reads the quota signal by running `program`; the seam tests script a fake CLI with.
pub fn usage_with(program: &Path) -> Result<UsageReport> {
    usage_with_timeout(program, USAGE_TIMEOUT)
}

/// Like [`usage_with`] but with an explicit time budget; the slow-path tests use it.
pub fn usage_with_timeout(program: &Path, timeout: Duration) -> Result<UsageReport> {
    let mut child = Command::new(program)
        .arg("usage")
        .arg("--json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AppError::new(
                ErrorCode::Io,
                format!(
                    "Kota okuması başlatılamadı ({}): {error}",
                    program.display()
                ),
            )
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "Kota çıktı akışı açılamadı."))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "Kota hata akışı açılamadı."))?;
    // Both pipes are drained on their own threads: a child that fills one before the parent
    // reads the other would otherwise block forever.
    let (output_tx, output_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::BufReader::new(stdout).read_to_end(&mut bytes);
        let _ = output_tx.send(bytes);
    });
    let (error_tx, error_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::BufReader::new(stderr).read_to_end(&mut bytes);
        let _ = error_tx.send(bytes);
    });
    let output = match output_rx.recv_timeout(timeout) {
        Ok(bytes) => bytes,
        Err(_) => {
            // A hung CLI must not hold the caller; the reader thread ends with the closed pipe.
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::new(
                ErrorCode::Io,
                format!(
                    "Kota okuması zaman aşımına uğradı ({} sn); Raios CLI yanıt vermedi.",
                    timeout.as_secs()
                ),
            ));
        }
    };
    let status = child.wait().map_err(|error| {
        AppError::new(ErrorCode::Io, format!("Kota beklemesi başarısız: {error}"))
    })?;
    if !status.success() {
        let errors = error_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_default();
        let message = match failure_detail(&errors) {
            Some(detail) => format!("Kota okuması başarısız: {detail}"),
            None => match status.code() {
                Some(code) => format!("Kota okuması başarısız (çıkış kodu {code})."),
                None => "Kota okuması başarısız.".to_owned(),
            },
        };
        return Err(AppError::new(ErrorCode::Io, message));
    }
    parse_report(&output)
}

/// The first interesting line of the CLI's stderr, without its own error prefix.
fn failure_detail(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let line = line.strip_prefix("usage failed: ").unwrap_or(line);
    Some(line.chars().take(200).collect())
}

/// Parses the CLI's answer into typed rows.
fn parse_report(bytes: &[u8]) -> Result<UsageReport> {
    let providers: Vec<UsageProvider> = serde_json::from_slice(bytes).map_err(|error| {
        AppError::new(
            ErrorCode::Internal,
            format!("Kota yanıtı okunamadı: {error}"),
        )
    })?;
    Ok(UsageReport { providers })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, remaining: Option<&str>) -> UsageProvider {
        UsageProvider {
            provider: name.to_owned(),
            installed: true,
            authenticated: true,
            remaining: remaining.map(str::to_owned),
        }
    }

    #[test]
    fn the_quota_line_joins_providers_with_a_remaining_figure() {
        let report = UsageReport {
            providers: vec![
                provider("Codex / OpenAI", None),
                provider("Claude Code", Some("5h:64% 7d:8% remaining")),
                provider("Antigravity CLI", Some("  ")),
            ],
        };
        assert_eq!(
            report.quota_line().as_deref(),
            Some("Kota: Claude Code 5h:64% 7d:8% remaining")
        );
    }

    #[test]
    fn a_report_without_figures_has_no_line() {
        let report = UsageReport {
            providers: vec![provider("Codex / OpenAI", None)],
        };
        assert_eq!(report.quota_line(), None);
        assert_eq!(
            UsageReport {
                providers: Vec::new()
            }
            .quota_line(),
            None
        );
    }

    #[test]
    fn rows_are_parsed_with_the_defaults_for_missing_flags() {
        let report = parse_report(br#"[{"provider":"Claude Code","remaining":"1% remaining"}]"#)
            .expect("parse");
        assert_eq!(report.providers.len(), 1);
        assert!(!report.providers[0].installed);
        assert_eq!(
            report.quota_line().as_deref(),
            Some("Kota: Claude Code 1% remaining")
        );
    }

    #[test]
    fn garbage_is_an_internal_error_not_a_panic() {
        let error = parse_report(b"not json").unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains("okunamadı"));
    }
}
