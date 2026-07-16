use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Problem {
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub retryable: bool,
}

impl Problem {
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
            retryable,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new("UNAUTHORIZED", msg, false)
    }

    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::new("INVALID_INPUT", msg, false)
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new("NOT_FOUND", msg, false)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", msg, true)
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new("FORBIDDEN", msg, false)
    }

    pub fn not_implemented(msg: impl Into<String>) -> Self {
        Self::new("NOT_IMPLEMENTED", msg, false)
    }

    pub fn duplicate_command(key: impl Into<String>) -> Self {
        Self::new(
            "DUPLICATE_COMMAND",
            format!("Command with idempotency key '{}' already processed", key.into()),
            false,
        )
    }
}
