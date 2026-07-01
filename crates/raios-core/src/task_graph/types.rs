use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSpec {
    pub id: String,
    pub description: String,
    pub shell_cmd: String,
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub graph_id: String,
    pub task_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub description: String,
    pub shell_cmd: String,
    pub deps: Vec<String>,
    pub status: NodeStatus,
    pub factory_job_id: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            NodeStatus::Pending => "pending",
            NodeStatus::Running => "running",
            NodeStatus::Completed => "completed",
            NodeStatus::Failed => "failed",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub id: String,
    pub goal: String,
    pub agent: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub nodes: Vec<GraphNode>,
}
