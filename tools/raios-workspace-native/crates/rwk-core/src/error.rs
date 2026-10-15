//! Stable error vocabulary shared by every rwk-core service.

use std::fmt;

/// Machine-readable error category. The UI branches on this, never on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// The caller supplied an invalid value.
    InvalidInput,
    /// The requested object does not exist or is not visible in this scope.
    NotFound,
    /// The object changed since the caller last read it.
    Conflict,
    /// A filesystem or OS call failed.
    Io,
    /// The daemon rejected the credentials.
    Unauthorized,
    /// The daemon or its policy forbids this operation.
    Forbidden,
    /// The daemon cannot be reached; mutations fail closed.
    DaemonUnreachable,
    /// The daemon returned an error this client does not model.
    Daemon,
    /// The workspace is shutting down; nothing new may start.
    WorkspaceClosed,
    /// An internal invariant failed.
    Internal,
}

impl ErrorCode {
    /// Every code, for exhaustive tests and documentation.
    pub const ALL: [ErrorCode; 10] = [
        ErrorCode::InvalidInput,
        ErrorCode::NotFound,
        ErrorCode::Conflict,
        ErrorCode::Io,
        ErrorCode::Unauthorized,
        ErrorCode::Forbidden,
        ErrorCode::DaemonUnreachable,
        ErrorCode::Daemon,
        ErrorCode::WorkspaceClosed,
        ErrorCode::Internal,
    ];

    /// Stable snake_case identifier.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidInput => "invalid_input",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Conflict => "conflict",
            ErrorCode::Io => "io_error",
            ErrorCode::Unauthorized => "unauthorized",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::DaemonUnreachable => "daemon_unreachable",
            ErrorCode::Daemon => "daemon_error",
            ErrorCode::WorkspaceClosed => "workspace_closed",
            ErrorCode::Internal => "internal_error",
        }
    }

    /// Maps a daemon `Problem.code` to the closest stable code. Codes this client does not
    /// model, including future ones, become [`ErrorCode::Daemon`].
    pub fn from_problem_code(code: &str) -> Self {
        match code {
            "INVALID_INPUT" => ErrorCode::InvalidInput,
            "NOT_FOUND" => ErrorCode::NotFound,
            "CONFLICT" => ErrorCode::Conflict,
            "UNAUTHORIZED" => ErrorCode::Unauthorized,
            "FORBIDDEN" => ErrorCode::Forbidden,
            "INTERNAL_ERROR" => ErrorCode::Internal,
            _ => ErrorCode::Daemon,
        }
    }
}

/// An error with a stable [`ErrorCode`] and a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    /// Stable category.
    pub code: ErrorCode,
    /// Message for the user or the log. Never contains terminal or clipboard content.
    pub message: String,
}

impl AppError {
    /// Creates an error from a code and a message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::new(ErrorCode::Io, error.to_string())
    }
}

/// Result alias used across rwk-core.
pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_code_has_a_unique_stable_identifier() {
        let identifiers: HashSet<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(identifiers.len(), ErrorCode::ALL.len());
        assert!(identifiers
            .iter()
            .all(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c == '_')));
    }

    #[test]
    fn known_daemon_problem_codes_map_to_their_stable_codes() {
        let cases = [
            ("INVALID_INPUT", ErrorCode::InvalidInput),
            ("NOT_FOUND", ErrorCode::NotFound),
            ("UNAUTHORIZED", ErrorCode::Unauthorized),
            ("FORBIDDEN", ErrorCode::Forbidden),
            ("INTERNAL_ERROR", ErrorCode::Internal),
        ];
        for (problem, expected) in cases {
            assert_eq!(ErrorCode::from_problem_code(problem), expected, "{problem}");
        }
    }

    #[test]
    fn unknown_daemon_problem_codes_never_become_a_more_specific_code() {
        for code in ["NOT_IMPLEMENTED", "DUPLICATE_COMMAND", "SOMETHING_NEW", ""] {
            assert_eq!(
                ErrorCode::from_problem_code(code),
                ErrorCode::Daemon,
                "{code:?}"
            );
        }
    }

    #[test]
    fn display_leads_with_the_stable_identifier() {
        let error = AppError::new(ErrorCode::Conflict, "file changed on disk");
        assert_eq!(error.to_string(), "conflict: file changed on disk");
    }

    #[test]
    fn io_errors_convert_with_the_io_code() {
        let error: AppError = std::io::Error::new(std::io::ErrorKind::NotFound, "gone").into();
        assert_eq!(error.code, ErrorCode::Io);
        assert!(error.message.contains("gone"));
    }
}
