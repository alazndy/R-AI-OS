use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "query_type", content = "payload")]
pub enum Query {
    GetSystemSnapshot,
    GetNowSnapshot,
    GetWorkSnapshot,
    GetExploreSnapshot {
        search_query: Option<String>,
        log_filter: Option<String>,
    },
    GetGovernSnapshot,
    GetProjectDetail {
        project_path: String,
    },
    GetTaskDetail {
        task_id: String,
    },
}
