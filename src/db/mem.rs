use super::*;
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
}

pub fn mem_upsert(
    conn: &Connection,
    project_key: &str,
    item_type: &str,
    slug: &str,
    title: &str,
    description: &str,
    body: &str,
    session_id: Option<&str>,
) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    conn.execute(
        "INSERT INTO mem_items
             (id, project_key, item_type, slug, title, description, body, created_at, updated_at, session_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8,?9)
         ON CONFLICT(project_key, slug) DO UPDATE SET
             item_type   = excluded.item_type,
             title       = excluded.title,
             description = excluded.description,
             body        = CASE
                             WHEN excluded.body != '' THEN mem_items.body || char(10) || '<!-- ' || ?8 || ' -->' || char(10) || excluded.body
                             ELSE mem_items.body
                           END,
             updated_at  = excluded.updated_at,
             session_id  = excluded.session_id",
        params![id, project_key, item_type, slug, title, description, body, now, session_id],
    )?;
    Ok(())
}

pub fn mem_list(conn: &Connection, project_key: &str) -> Result<Vec<MemItemRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_key, item_type, slug, title, description, body, created_at, updated_at, session_id
         FROM mem_items WHERE project_key = ?1 ORDER BY item_type, slug",
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
            })
        })?
        .flatten()
        .collect();
    Ok(rows)
}

pub fn mem_get(conn: &Connection, project_key: &str, slug: &str) -> Result<Option<MemItemRow>> {
    conn.query_row(
        "SELECT id, project_key, item_type, slug, title, description, body, created_at, updated_at, session_id
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
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

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
            "---\nname: {}\ndescription: {}\nmetadata:\n  type: {}\n---\n\n{}\n",
            item.slug, item.description, item.item_type, item.body
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
    let entries: Vec<String> = items
        .iter()
        .map(|i| format!("- [{}]({}.md) — {}", i.title, i.slug, i.description))
        .collect();
    let content = format!("{}\n\n{}\n", header, entries.join("\n"));
    let _ = std::fs::write(&index_path, content);

    Ok(items.len())
}
