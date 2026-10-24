//! Operator tasks: the checks done before a task command leaves the workspace, and the status
//! words the daemon accepts. The daemon validates again; these checks exist so an obviously bad
//! request never leaves and the user sees why.

use crate::error::{AppError, ErrorCode, Result};
use crate::handoff_input::looks_like_secret;

/// Longest task title the daemon stores, in characters.
pub const MAX_TITLE_CHARS: usize = 240;

/// Checks a task title typed by the user and returns it trimmed.
pub fn validate_title(raw: &str) -> Result<String> {
    let title = raw.trim();
    let refuse = |message: String| Err(AppError::new(ErrorCode::InvalidInput, message));
    if title.is_empty() {
        return refuse("Görev başlığı boş olamaz.".to_owned());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return refuse(format!(
            "Görev başlığı en fazla {MAX_TITLE_CHARS} karakter olabilir."
        ));
    }
    if title.chars().any(char::is_control) {
        return refuse("Görev başlığı denetim karakteri içeremez.".to_owned());
    }
    if looks_like_secret(title) {
        return refuse("Başlık bir kimlik bilgisi gibi görünüyor; değiştirin.".to_owned());
    }
    Ok(title.to_owned())
}

/// The statuses an operator can set from a control surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Waiting to be picked up.
    Queued,
    /// Being worked on.
    InProgress,
    /// Waiting for something.
    Blocked,
    /// Done.
    Completed,
    /// Dropped.
    Cancelled,
}

impl TaskStatus {
    /// The word the daemon takes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    /// The word shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "sırada",
            Self::InProgress => "sürüyor",
            Self::Blocked => "engelli",
            Self::Completed => "bitti",
            Self::Cancelled => "iptal",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_trimmed_and_the_unsafe_ones_are_refused() {
        assert_eq!(
            validate_title("  Fix the build \n").unwrap(),
            "Fix the build"
        );
        for bad in [
            "",
            "   ",
            "two\nlines",
            "nul\0",
            "key sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
        ] {
            assert_eq!(
                validate_title(bad).unwrap_err().code,
                ErrorCode::InvalidInput,
                "{bad:?}"
            );
        }
        assert!(validate_title(&"é".repeat(MAX_TITLE_CHARS)).is_ok());
        assert!(validate_title(&"é".repeat(MAX_TITLE_CHARS + 1)).is_err());
    }

    #[test]
    fn statuses_use_the_daemons_words() {
        let words: Vec<&str> = [
            TaskStatus::Queued,
            TaskStatus::InProgress,
            TaskStatus::Blocked,
            TaskStatus::Completed,
            TaskStatus::Cancelled,
        ]
        .into_iter()
        .map(TaskStatus::as_str)
        .collect();
        assert_eq!(
            words,
            ["queued", "in_progress", "blocked", "completed", "cancelled"]
        );
    }
}
