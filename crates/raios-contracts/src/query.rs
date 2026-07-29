//! Control-plane query definition contracts.

use serde::{Deserialize, Serialize};

/// Typed read-only queries requested from the resident daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "query_type", content = "payload")]
pub enum Query {
    /// Retrieve the complete system-wide snapshot envelope.
    GetSystemSnapshot,
    /// Retrieve the Now route snapshot.
    GetNowSnapshot,
    /// Retrieve the Work route snapshot.
    GetWorkSnapshot,
    /// Retrieve the Explore route snapshot with optional search and log filters.
    GetExploreSnapshot {
        /// Optional search query term to filter results.
        search_query: Option<String>,
        /// Optional category log filter term.
        log_filter: Option<String>,
    },
    /// Retrieve the Govern route snapshot.
    GetGovernSnapshot,
    /// Retrieve detailed posture data for a specific project.
    GetProjectDetail {
        /// Absolute path to the project root directory.
        project_path: String,
    },
    /// Retrieve detailed information for a specific task.
    GetTaskDetail {
        /// Unique task identifier.
        task_id: String,
    },
}
