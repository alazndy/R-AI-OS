use super::*;

#[test]
fn mem_schema_has_layer_nodes_lineage() {
    let conn = in_memory();
    // layer column exists with default 2
    conn.execute(
        "INSERT INTO mem_items (id, project_key, item_type, slug, title) VALUES ('x','p','project','s','T')",
        [],
    )
    .unwrap();
    let layer: i64 = conn
        .query_row("SELECT layer FROM mem_items WHERE id='x'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(layer, 2);
    // mem_nodes and mem_lineage exist
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_nodes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    let l: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_lineage", [], |r| r.get(0))
        .unwrap();
    assert_eq!(l, 0);
}

#[test]
fn mem_node_add_and_lineage_round_trip() {
    let conn = in_memory();
    let node_id =
        mem_node_add(&conn, "-home-alaz-p", "l0_raw", "claude", "User: raw line", None).unwrap();
    assert!(!node_id.is_empty());

    mem_lineage_add(&conn, "item", "item-1", "node", &node_id, "derived_from").unwrap();
    // idempotent: second insert must not error
    mem_lineage_add(&conn, "item", "item-1", "node", &node_id, "derived_from").unwrap();

    let parents = mem_lineage_parents(&conn, "item", "item-1").unwrap();
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0], ("node".to_string(), node_id, "derived_from".to_string()));
}

#[test]
fn mem_history_returns_revision_nodes_newest_first() {
    let conn = in_memory();
    let key = "-home-alaz-p";
    mem_upsert(
        &conn,
        MemUpsert {
            project_key: key,
            item_type: "project",
            slug: "arch",
            title: "Arch",
            description: "d",
            body: "v1",
            session_id: None,
            layer: 1,
        },
    )
    .unwrap();
    let item = mem_get(&conn, key, "arch").unwrap().unwrap();
    let n1 = mem_node_add(&conn, key, "revision", "2026-07-08", "old body v0", None).unwrap();
    mem_lineage_add(&conn, "item", &item.id, "node", &n1, "revision").unwrap();

    let hist = mem_history(&conn, key, "arch").unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].content, "old body v0");
    assert_eq!(hist[0].kind, "revision");

    // unknown slug → empty, no error
    assert!(mem_history(&conn, key, "nope").unwrap().is_empty());
}

#[test]
fn mem_upsert_replaces_body_and_archives_revision() {
    let conn = in_memory();
    let key = "-home-alaz-p";
    let up = |body: &'static str| MemUpsert {
        project_key: key,
        item_type: "feedback",
        slug: "rule-x",
        title: "Rule X",
        description: "d",
        body,
        session_id: None,
        layer: 1,
    };
    mem_upsert(&conn, up("first version")).unwrap();
    mem_upsert(&conn, up("second version")).unwrap();

    let item = mem_get(&conn, key, "rule-x").unwrap().unwrap();
    // body is REPLACED, never concatenated
    assert_eq!(item.body, "second version");
    assert_eq!(item.layer, 1);

    // old body archived as revision node
    let hist = mem_history(&conn, key, "rule-x").unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].content, "first version");
}

#[test]
fn mem_upsert_identical_or_empty_body_creates_no_revision() {
    let conn = in_memory();
    let key = "-home-alaz-p";
    let up = |body: &'static str| MemUpsert {
        project_key: key,
        item_type: "project",
        slug: "s",
        title: "T",
        description: "",
        body,
        session_id: None,
        layer: 1,
    };
    mem_upsert(&conn, up("same")).unwrap();
    mem_upsert(&conn, up("same")).unwrap(); // identical → no revision
    mem_upsert(&conn, up("")).unwrap();     // empty → keep body, no revision

    let item = mem_get(&conn, key, "s").unwrap().unwrap();
    assert_eq!(item.body, "same");
    assert!(mem_history(&conn, key, "s").unwrap().is_empty());
}
