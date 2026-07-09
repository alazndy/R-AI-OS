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
