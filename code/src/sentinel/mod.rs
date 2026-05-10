use serde::{Deserialize, Serialize};

pub mod monitor;
pub mod compiler;
pub mod tester;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SentinelState {
    /// No changes detected since last verification.
    Clean,
    /// File modified, awaiting check.
    Dirty,
    /// Compilation/Check in progress.
    Compiling,
    /// Syntax or type errors found.
    Failed,
    /// Compiled successfully.
    Compiled,
    /// Compiled and tests passed.
    Verified,
}

impl Default for SentinelState {
    fn default() -> Self {
        Self::Clean
    }
}
