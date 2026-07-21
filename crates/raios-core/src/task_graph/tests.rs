use super::*;
use rusqlite::params;

fn make_store(tmp: &tempfile::TempDir) -> GraphStore {
    GraphStore::new(tmp.path().join("test.db"))
}

#[test]
fn create_graph_and_list_nodes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = make_store(&tmp);
    let nodes = vec![
        NodeSpec {
            id: "a".into(),
            description: "build".into(),
            shell_cmd: "cargo build".into(),
            deps: vec![],
        },
        NodeSpec {
            id: "b".into(),
            description: "test".into(),
            shell_cmd: "cargo test".into(),
            deps: vec!["a".into()],
        },
    ];
    let graph_id = store.create("build and test", "claude", nodes).unwrap();
    let graph = store.get(&graph_id).unwrap();
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph.nodes.iter().all(|node| node.task_id.is_some()));
    assert!(graph.nodes.iter().all(|node| node.agent_run_id.is_some()));
    let test_node = graph.nodes.iter().find(|node| node.id == "b").unwrap();
    assert_eq!(test_node.deps, vec!["a"]);
}

#[test]
fn ready_nodes_excludes_nodes_with_pending_deps() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = make_store(&tmp);
    let nodes = vec![
        NodeSpec {
            id: "a".into(),
            description: "first".into(),
            shell_cmd: "echo a".into(),
            deps: vec![],
        },
        NodeSpec {
            id: "b".into(),
            description: "second".into(),
            shell_cmd: "echo b".into(),
            deps: vec!["a".into()],
        },
    ];
    let gid = store.create("goal", "claude", nodes).unwrap();
    let ready = store.ready_nodes(&gid);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, "a");
}

#[test]
fn node_count_enforces_max_limit() {
    let too_many: Vec<NodeSpec> = (0..51)
        .map(|i| NodeSpec {
            id: i.to_string(),
            description: format!("task {i}"),
            shell_cmd: "echo x".into(),
            deps: vec![],
        })
        .collect();
    let tmp = tempfile::TempDir::new().unwrap();
    let store = make_store(&tmp);
    assert!(store.create("too big", "claude", too_many).is_err());
}

#[test]
fn mark_node_complete_unlocks_dependent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = make_store(&tmp);
    let nodes = vec![
        NodeSpec {
            id: "a".into(),
            description: "a".into(),
            shell_cmd: "echo a".into(),
            deps: vec![],
        },
        NodeSpec {
            id: "b".into(),
            description: "b".into(),
            shell_cmd: "echo b".into(),
            deps: vec!["a".into()],
        },
    ];
    let gid = store.create("test", "claude", nodes).unwrap();
    store.mark_node_complete(&gid, "a", "ok", "job-1");
    let ready = store.ready_nodes(&gid);
    assert_eq!(ready[0].id, "b");

    let conn = store.connect().unwrap();
    let task_id = ready[0].task_id.clone().unwrap();
    let control_status: String = conn
        .query_row(
            "SELECT status FROM cp_tasks WHERE id=?1",
            params![task_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(control_status, "ready");
}

#[test]
fn graph_status_follows_control_plane_lifecycle() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = make_store(&tmp);
    let nodes = vec![NodeSpec {
        id: "a".into(),
        description: "a".into(),
        shell_cmd: "echo a".into(),
        deps: vec![],
    }];
    let gid = store.create("test", "claude", nodes).unwrap();

    store.mark_node_running(&gid, "a", "job-1");
    let running = store.get(&gid).unwrap();
    assert_eq!(running.status, "running");
    assert_eq!(running.nodes[0].status, NodeStatus::Running);

    store.mark_node_complete(&gid, "a", "ok", "job-1");
    let completed = store.get(&gid).unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(completed.nodes[0].status, NodeStatus::Completed);
}

#[test]
fn legacy_graph_cache_is_rebuilt_from_canonical_state() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = make_store(&tmp);
    let nodes = vec![NodeSpec {
        id: "a".into(),
        description: "a".into(),
        shell_cmd: "echo a".into(),
        deps: vec![],
    }];
    let gid = store.create("test", "claude", nodes).unwrap();

    store.mark_node_running(&gid, "a", "job-1");
    let conn = store.connect().unwrap();
    let legacy_status: String = conn
        .query_row(
            "SELECT status FROM task_graph_nodes WHERE graph_id=?1 AND id=?2",
            params![gid, "a"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy_status, "running");

    conn.execute(
        "DELETE FROM task_graph_nodes WHERE graph_id=?1",
        params![gid],
    )
    .unwrap();
    store.refresh_legacy_cache(&gid, &conn);

    let rebuilt_status: String = conn
        .query_row(
            "SELECT status FROM task_graph_nodes WHERE graph_id=?1 AND id=?2",
            params![gid, "a"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rebuilt_status, "running");

    let rebuilt = store.get(&gid).unwrap();
    assert_eq!(rebuilt.nodes[0].status, NodeStatus::Running);
}

#[test]
fn cycle_detection_rejects_cyclic_graph() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = make_store(&tmp);
    let nodes = vec![
        NodeSpec {
            id: "a".into(),
            description: "a".into(),
            shell_cmd: "echo".into(),
            deps: vec!["b".into()],
        },
        NodeSpec {
            id: "b".into(),
            description: "b".into(),
            shell_cmd: "echo".into(),
            deps: vec!["a".into()],
        },
    ];
    assert!(store.create("cyclic", "claude", nodes).is_err());
}
