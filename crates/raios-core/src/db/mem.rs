use rusqlite::{params, Connection, OptionalExtension, Result};
// ─── Memory Items (mem_items) ─────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemItemRow {
    pub id: String,
    pub project_key: String,
    pub item_type: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    pub session_id: Option<String>,
    pub layer: i64,
}

pub struct MemUpsert<'a> {
    pub project_key: &'a str,
    pub item_type: &'a str,
    pub slug: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub body: &'a str,
    pub session_id: Option<&'a str>,
    pub layer: i64,
}

pub fn mem_upsert(conn: &Connection, item: MemUpsert) -> Result<()> {
    let MemUpsert {
        project_key,
        item_type,
        slug,
        title,
        description,
        body,
        session_id,
        layer,
    } = item;

    // Archive the previous body as an immutable revision node before replacing.
    if !body.is_empty() {
        if let Some(prev) = mem_get(conn, project_key, slug)? {
            if !prev.body.is_empty() && prev.body != body {
                let node_id = mem_node_add(
                    conn,
                    project_key,
                    "revision",
                    &prev.updated_at,
                    &prev.body,
                    prev.session_id.as_deref(),
                )?;
                mem_lineage_add(conn, "item", &prev.id, "node", &node_id, "revision")?;
            }
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    conn.execute(
        "INSERT INTO mem_items
             (id, project_key, item_type, slug, title, description, body, created_at, updated_at, session_id, layer)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8,?9,?10)
         ON CONFLICT(project_key, slug) DO UPDATE SET
             item_type   = excluded.item_type,
             title       = excluded.title,
             description = excluded.description,
             body        = CASE
                             WHEN excluded.body != '' THEN excluded.body
                             ELSE mem_items.body
                           END,
             updated_at  = excluded.updated_at,
             session_id  = excluded.session_id,
             layer       = excluded.layer",
        params![id, project_key, item_type, slug, title, description, body, now, session_id, layer],
    )?;
    Ok(())
}

pub fn mem_list(conn: &Connection, project_key: &str) -> Result<Vec<MemItemRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_key, item_type, slug, title, description, body, created_at, updated_at, session_id, layer
         FROM mem_items WHERE project_key = ?1 ORDER BY layer DESC, item_type, slug",
    )?;
    let rows = stmt
        .query_map(params![project_key], |row| {
            Ok(MemItemRow {
                id: row.get(0)?,
                project_key: row.get(1)?,
                item_type: row.get(2)?,
                slug: row.get(3)?,
                title: row.get(4)?,
                description: row.get(5)?,
                body: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                session_id: row.get(9)?,
                layer: row.get(10)?,
            })
        })?
        .flatten()
        .collect();
    Ok(rows)
}

pub fn mem_get(conn: &Connection, project_key: &str, slug: &str) -> Result<Option<MemItemRow>> {
    conn.query_row(
        "SELECT id, project_key, item_type, slug, title, description, body, created_at, updated_at, session_id, layer
         FROM mem_items WHERE project_key = ?1 AND slug = ?2",
        params![project_key, slug],
        |row| {
            Ok(MemItemRow {
                id: row.get(0)?,
                project_key: row.get(1)?,
                item_type: row.get(2)?,
                slug: row.get(3)?,
                title: row.get(4)?,
                description: row.get(5)?,
                body: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                session_id: row.get(9)?,
                layer: row.get(10)?,
            })
        },
    )
    .optional()}

pub fn mem_delete(conn: &Connection, project_key: &str, slug: &str) -> Result<bool> {
    let n = conn.execute(
        "DELETE FROM mem_items WHERE project_key = ?1 AND slug = ?2",
        params![project_key, slug],
    )?;
    Ok(n > 0)
}

/// Write all DB mem_items for a project to `~/.claude/projects/<key>/memory/`
/// as individual markdown files and rebuild MEMORY.md index.
/// Returns the number of files written.
pub fn mem_export(conn: &Connection, project_key: &str, memory_dir: &std::path::Path) -> Result<usize> {
    let items = mem_list(conn, project_key)?;
    if items.is_empty() {
        return Ok(0);
    }
    let _ = std::fs::create_dir_all(memory_dir);

    for item in &items {
        let file_path = memory_dir.join(format!("{}.md", item.slug));
        let content = format!(
            "---\nname: {}\ndescription: {}\nmetadata:\n  type: {}\n  layer: {}\n---\n\n{}\n",
            item.slug, item.description, item.item_type, item.layer, item.body
        );
        let _ = std::fs::write(&file_path, content);
    }

    // Rebuild MEMORY.md index
    let index_path = memory_dir.join("MEMORY.md");
    let existing = std::fs::read_to_string(&index_path).unwrap_or_default();
    let header = if existing.starts_with("# Memory Index") {
        existing.lines().next().unwrap_or("# Memory Index").to_string()
    } else {
        format!(
            "# Memory Index — {}",
            project_key.trim_start_matches('-').replace('-', " ")
        )
    };
    let section = |layer: i64, heading: &str| -> String {
        let lines: Vec<String> = items
            .iter()
            .filter(|i| i.layer == layer)
            .map(|i| format!("- [{}]({}.md) — {}", i.title, i.slug, i.description))
            .collect();
        if lines.is_empty() {
            String::new()
        } else {
            format!("\n## {}\n{}\n", heading, lines.join("\n"))
        }
    };
    let content = format!(
        "{}\n{}{}{}",
        header,
        section(3, "Persona (L3)"),
        section(2, "Scenes (L2)"),
        section(1, "Facts (L1)"),
    );
    let _ = std::fs::write(&index_path, content);

    Ok(items.len())
}

// ─── Memory Nodes (mem_nodes) & Lineage (mem_lineage) ────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemNodeRow {
    pub id: String,
    pub project_key: String,
    pub kind: String,
    pub source: String,
    pub content: String,
    pub session_id: Option<String>,
    pub created_at: String,
}

/// Insert an immutable evidence node (L0 raw excerpt or archived revision).
/// Returns the generated node id.
pub fn mem_node_add(
    conn: &Connection,
    project_key: &str,
    kind: &str,
    source: &str,
    content: &str,
    session_id: Option<&str>,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "INSERT INTO mem_nodes (id, project_key, kind, source, content, session_id, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![id, project_key, kind, source, content, session_id, now],
    )?;
    Ok(id)
}

pub fn mem_node_get(conn: &Connection, id: &str) -> Result<Option<MemNodeRow>> {
    conn.query_row(
        "SELECT id, project_key, kind, source, content, session_id, created_at
         FROM mem_nodes WHERE id = ?1",
        params![id],
        |row| {
            Ok(MemNodeRow {
                id: row.get(0)?,
                project_key: row.get(1)?,
                kind: row.get(2)?,
                source: row.get(3)?,
                content: row.get(4)?,
                session_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )
    .optional()
}

/// Record a derived-from / revision edge. Idempotent.
pub fn mem_lineage_add(
    conn: &Connection,
    child_kind: &str,
    child_id: &str,
    parent_kind: &str,
    parent_id: &str,
    relation: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO mem_lineage (child_kind, child_id, parent_kind, parent_id, relation)
         VALUES (?1,?2,?3,?4,?5)",
        params![child_kind, child_id, parent_kind, parent_id, relation],
    )?;
    Ok(())
}

/// All parents of a child: (parent_kind, parent_id, relation), oldest first.
pub fn mem_lineage_parents(
    conn: &Connection,
    child_kind: &str,
    child_id: &str,
) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT parent_kind, parent_id, relation FROM mem_lineage
         WHERE child_kind = ?1 AND child_id = ?2 ORDER BY id",
    )?;
    let rows = stmt
        .query_map(params![child_kind, child_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .flatten()
        .collect();
    Ok(rows)
}

/// Archived body revisions of a mem_item, newest first. Empty vec for unknown slug.
pub fn mem_history(conn: &Connection, project_key: &str, slug: &str) -> Result<Vec<MemNodeRow>> {
    let Some(item) = mem_get(conn, project_key, slug)? else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT n.id, n.project_key, n.kind, n.source, n.content, n.session_id, n.created_at
         FROM mem_nodes n
         JOIN mem_lineage l ON l.parent_kind = 'node' AND l.parent_id = n.id
         WHERE l.child_kind = 'item' AND l.child_id = ?1 AND l.relation = 'revision'
         ORDER BY n.created_at DESC, n.id DESC",
    )?;
    let rows = stmt
        .query_map(params![item.id], |row| {
            Ok(MemNodeRow {
                id: row.get(0)?,
                project_key: row.get(1)?,
                kind: row.get(2)?,
                source: row.get(3)?,
                content: row.get(4)?,
                session_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
        .flatten()
        .collect();
    Ok(rows)
}
