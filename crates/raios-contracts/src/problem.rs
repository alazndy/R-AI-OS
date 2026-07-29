//! Structured error contracts for control-plane operation failures.

use serde::{Deserialize, Serialize};

/// Structured error payload returned when a control-plane command or query fails.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Problem {
    /// Categorical error code string (e.g., `"UNAUTHORIZED"`, `"NOT_FOUND"`).
    pub code: String,
    /// Human-readable error description message.
    pub message: String,
    /// Optional arbitrary JSON details attached to the error.
    pub details: Option<serde_json::Value>,
    /// Whether retrying the request with the same parameters may succeed.
    pub retryable: bool,
}

impl Problem {
    /// Creates a new `Problem` with the specified code, message, and retryable flag.
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
            retryable,
        }
    }

    /// Attaches structured JSON details to this error payload.
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Creates an `UNAUTHORIZED` non-retryable error.
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new("UNAUTHORIZED", msg, false)
    }

    /// Creates an `INVALID_INPUT` non-retryable error.
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::new("INVALID_INPUT", msg, false)
    }

    /// Creates a `NOT_FOUND` non-retryable error.
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new("NOT_FOUND", msg, false)
    }

    /// Creates an `INTERNAL_ERROR` retryable error.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", msg, true)
    }

    /// Creates a `FORBIDDEN` non-retryable error.
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new("FORBIDDEN", msg, false)
    }

    /// Creates a `NOT_IMPLEMENTED` non-retryable error.
    pub fn not_implemented(msg: impl Into<String>) -> Self {
        Self::new("NOT_IMPLEMENTED", msg, false)
    }

    /// Creates a `DUPLICATE_COMMAND` non-retryable error for duplicate idempotency keys.
    pub fn duplicate_command(key: impl Into<String>) -> Self {
        Self::new(
            "DUPLICATE_COMMAND",
            format!(
                "Command with idempotency key '{}' already processed",
                key.into()
            ),
            false,
        )
    }
}
