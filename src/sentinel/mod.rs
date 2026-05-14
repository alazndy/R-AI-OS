use serde::{Deserialize, Serialize};

pub mod compiler;
pub mod monitor;
pub mod tester;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SentinelState {
    /// No changes detected since last verification.
    #[default]
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
