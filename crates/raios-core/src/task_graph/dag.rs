use super::types::{NodeSpec, NodeStatus};
use anyhow::{bail, Result};

pub(super) fn validate_dag(nodes: &[NodeSpec]) -> Result<()> {
    let ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    for node in nodes {
        for dep in &node.deps {
            if !ids.contains(dep.as_str()) {
                bail!(
                    "Node '{}' depends on '{}' which is not in the graph",
                    node.id,
                    dep
                );
            }
            if dep == &node.id {
                bail!("Node '{}' depends on itself", node.id);
            }
        }
    }

    let mut visited = std::collections::HashSet::new();
    let mut in_stack = std::collections::HashSet::new();
    let adj: std::collections::HashMap<&str, Vec<&str>> = nodes
        .iter()
        .map(|n| (n.id.as_str(), n.deps.iter().map(|d| d.as_str()).collect()))
        .collect();

    for node in nodes {
        if has_cycle(&adj, node.id.as_str(), &mut visited, &mut in_stack) {
            bail!("Task graph contains a cycle involving '{}'", node.id);
        }
    }
    Ok(())
}

fn has_cycle<'a>(
    adj: &std::collections::HashMap<&'a str, Vec<&'a str>>,
    node: &'a str,
    visited: &mut std::collections::HashSet<&'a str>,
    in_stack: &mut std::collections::HashSet<&'a str>,
) -> bool {
    if in_stack.contains(node) {
        return true;
    }
    if visited.contains(node) {
        return false;
    }
    visited.insert(node);
    in_stack.insert(node);
    if let Some(deps) = adj.get(node) {
        for dep in deps {
            if has_cycle(adj, dep, visited, in_stack) {
                return true;
            }
        }
    }
    in_stack.remove(node);
    false
}

pub(super) fn legacy_status_to_node_status(status: &str) -> NodeStatus {
    match status {
        "running" => NodeStatus::Running,
        "completed" => NodeStatus::Completed,
        "failed" => NodeStatus::Failed,
        _ => NodeStatus::Pending,
    }
}

pub(super) fn control_status_to_node_status(status: &str) -> NodeStatus {
    match status {
        "running" => NodeStatus::Running,
        "completed" => NodeStatus::Completed,
        "failed" | "cancelled" => NodeStatus::Failed,
        _ => NodeStatus::Pending,
    }
}
