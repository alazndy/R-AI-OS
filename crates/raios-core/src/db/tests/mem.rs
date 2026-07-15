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
            provenance: None,
            confidence: None,
            last_used_at: None,
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
        provenance: None,
        confidence: None,
        last_used_at: None,
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
        provenance: None,
        confidence: None,
        last_used_at: None,
    };
    mem_upsert(&conn, up("same")).unwrap();
    mem_upsert(&conn, up("same")).unwrap(); // identical → no revision
    mem_upsert(&conn, up("")).unwrap();     // empty → keep body, no revision

    let item = mem_get(&conn, key, "s").unwrap().unwrap();
    assert_eq!(item.body, "same");
    assert!(mem_history(&conn, key, "s").unwrap().is_empty());
}

#[test]
fn mem_node_add_dedupes_l0_raw_content_within_project() {
    let conn = in_memory();
    let key = "-home-alaz-p";
    let id1 = mem_node_add(&conn, key, "l0_raw", "claude", "User: don't use npm here", None).unwrap();
    let id2 = mem_node_add(&conn, key, "l0_raw", "claude", "User: don't use npm here", None).unwrap();

    assert_eq!(id1, id2, "repeated l0_raw content must return the SAME node id, not a new one");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_nodes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "repeated l0_raw content must not insert a duplicate row");
}

#[test]
fn mem_node_add_still_inserts_fresh_revision_nodes_each_time() {
    let conn = in_memory();
    let key = "-home-alaz-p";
    let id1 = mem_node_add(&conn, key, "revision", "2026-07-08", "same old body", None).unwrap();
    let id2 = mem_node_add(&conn, key, "revision", "2026-07-08", "same old body", None).unwrap();

    assert_ne!(id1, id2, "revision nodes must always be inserted fresh, even with identical content");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_nodes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2, "two distinct revision snapshots must both be present");
}

#[test]
fn mem_upsert_rolls_back_revision_archiving_when_final_insert_fails() {
    let conn = in_memory();
    let key = "-home-alaz-p";
    mem_upsert(
        &conn,
        MemUpsert {
            project_key: key,
            item_type: "feedback",
            slug: "rule-y",
            title: "Rule Y",
            description: "d",
            body: "first version",
            session_id: None,
            layer: 1,
            provenance: None,
            confidence: None,
            last_used_at: None,
        },
    )
    .unwrap();

    // Second call changes the body (which would trigger revision archiving:
    // mem_node_add("revision", ...) + mem_lineage_add) but uses an item_type
    // that violates mem_items' CHECK(item_type IN (...)) constraint, so the
    // final INSERT ... ON CONFLICT UPDATE fails. Without a transaction wrapping
    // the whole sequence, the revision node + lineage edge from the earlier,
    // separately-committed statements would be left behind as orphans even
    // though the item's body was never actually replaced.
    let result = mem_upsert(
        &conn,
        MemUpsert {
            project_key: key,
            item_type: "not-a-real-type",
            slug: "rule-y",
            title: "Rule Y",
            description: "d",
            body: "second version",
            session_id: None,
            layer: 1,
            provenance: None,
            confidence: None,
            last_used_at: None,
        },
    );
    assert!(result.is_err(), "invalid item_type must fail the final insert/update");

    // Body must be untouched.
    let item = mem_get(&conn, key, "rule-y").unwrap().unwrap();
    assert_eq!(item.body, "first version");

    // No orphaned revision node or lineage edge from the failed attempt.
    let hist = mem_history(&conn, key, "rule-y").unwrap();
    assert!(
        hist.is_empty(),
        "revision archiving must roll back atomically with the failed insert, got: {:?}",
        hist
    );
    let node_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_nodes WHERE kind = 'revision'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(node_count, 0, "orphaned revision node must not survive a rolled-back mem_upsert");
}

#[test]
fn mem_export_groups_by_layer_persona_first() {
    let conn = in_memory();
    let key = "-home-alaz-p";
    for (slug, layer, t) in [("persona", 3, "user"), ("scene-20260709", 2, "project"), ("feedback-abc", 1, "feedback")] {
        mem_upsert(&conn, MemUpsert {
            project_key: key, item_type: t, slug, title: slug,
            description: "d", body: "b", session_id: None, layer,
            provenance: None, confidence: None, last_used_at: None,
        }).unwrap();
    }
    let dir = std::env::temp_dir().join(format!("raios-mem-test-{}", std::process::id()));
    let n = mem_export(&conn, key, &dir).unwrap();
    assert_eq!(n, 3);

    let persona_md = std::fs::read_to_string(dir.join("persona.md")).unwrap();
    assert!(persona_md.contains("  layer: 3"));

    let index = std::fs::read_to_string(dir.join("MEMORY.md")).unwrap();
    let p3 = index.find("## Persona (L3)").unwrap();
    let p2 = index.find("## Scenes (L2)").unwrap();
    let p1 = index.find("## Facts (L1)").unwrap();
    assert!(p3 < p2 && p2 < p1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mem_provenance_and_decay() {
    let conn = in_memory();
    let key = "-home-alaz-p";

    mem_upsert(
        &conn,
        MemUpsert {
            project_key: key,
            item_type: "project",
            slug: "decay-test",
            title: "Decay Test",
            description: "testing lazy confidence decay",
            body: "content",
            session_id: None,
            layer: 1,
            provenance: Some(Provenance::UserStated),
            confidence: Some(1.0),
            last_used_at: Some("2026-06-01 12:00:00"),
        },
    )
    .unwrap();

    let item = mem_get(&conn, key, "decay-test").unwrap().unwrap();
    assert_eq!(item.provenance, Provenance::UserStated);
    assert_eq!(item.confidence, 1.0);

    // Calculate effective confidence at a date 30 days after last_used_at (30-day half-life for project items)
    let at_30_days = chrono::NaiveDateTime::parse_from_str("2026-07-01 12:00:00", "%Y-%m-%d %H:%M:%S")
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap();
    let eff = item.effective_confidence_at(at_30_days);
    // 1 half-life = 0.5
    assert!((eff - 0.5).abs() < 0.01, "expected ~0.5 effective confidence after 1 half-life, got {}", eff);

    // Test mem_on_used
    mem_on_used(&conn, key, "decay-test").unwrap();
    let updated_item = mem_get(&conn, key, "decay-test").unwrap().unwrap();
    assert!(updated_item.last_used_at.as_deref().unwrap() > "2026-06-01 12:00:00");
}

