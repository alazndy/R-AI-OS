use rusqlite::{params, Connection, OptionalExtension, Result};

// ─── Memory Items (mem_items) ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    UserStated,
    Observed,
    Inferred,
    Corrected,
}

impl Provenance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Provenance::UserStated => "user_stated",
            Provenance::Observed => "observed",
            Provenance::Inferred => "inferred",
            Provenance::Corrected => "corrected",
        }
    }
}

impl std::fmt::Display for Provenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Provenance {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "user_stated" | "userstated" | "user" => Provenance::UserStated,
            "inferred" => Provenance::Inferred,
            "corrected" => Provenance::Corrected,
            _ => Provenance::Observed,
        })
    }
}

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
    pub provenance: Provenance,
    pub confidence: f64,
    pub last_used_at: Option<String>,
}

impl MemItemRow {
    /// Calculate lazy confidence decay based on time elapsed since `last_used_at` (or fallback `updated_at`/`created_at`).
    /// Half-life depends on `item_type`:
    /// - feedback: 90 days
    /// - project: 30 days
    /// - user: 120 days
    /// - reference: 60 days
    /// - default: 30 days
    pub fn effective_confidence_at(&self, at: chrono::DateTime<chrono::Local>) -> f64 {
        let reference_str = self
            .last_used_at
            .as_deref()
            .unwrap_or(&self.updated_at);

        let parsed_time = chrono::NaiveDateTime::parse_from_str(reference_str, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(reference_str, "%Y-%m-%dT%H:%M:%SZ"))
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&self.created_at, "%Y-%m-%d %H:%M:%S"))
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&self.created_at, "%Y-%m-%dT%H:%M:%SZ"))
            .ok();

        let Some(ref_dt) = parsed_time else {
            return self.confidence;
        };

        let ref_local = ref_dt.and_local_timezone(chrono::Local).latest().unwrap_or(at);
        let elapsed_secs = at.timestamp() - ref_local.timestamp();

        if elapsed_secs <= 0 {
            return self.confidence;
        }

        let half_life_days: f64 = match self.item_type.as_str() {
            "feedback" => 90.0,
            "user" => 120.0,
            "reference" => 60.0,
            "project" => 30.0,
            _ => 30.0,
        };

        let elapsed_days = elapsed_secs as f64 / 86400.0;
        let decayed = self.confidence * 0.5f64.powf(elapsed_days / half_life_days);
        decayed.clamp(0.0, 1.0)
    }

    pub fn effective_confidence(&self) -> f64 {
        self.effective_confidence_at(chrono::Local::now())
    }
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
    pub provenance: Option<Provenance>,
    pub confidence: Option<f64>,
    pub last_used_at: Option<&'a str>,
}

/// Archive-then-replace a mem_item's body. Runs the revision-archiving check
/// (`mem_node_add` for the "revision" node + `mem_lineage_add`) and the final
/// `INSERT ... ON CONFLICT` as a single atomic transaction: a crash or error
/// partway through (e.g. a constraint violation on the final insert) rolls
/// back the whole sequence rather than leaving an orphaned revision node or a
/// body that wasn't actually archived. `mem_upsert` takes `&Connection`
/// (shared, not `&mut`) to match every existing call site in this codebase,
/// so this uses `Connection::unchecked_transaction()` rather than
/// `Connection::transaction()` (which requires `&mut Connection`) — safe here
/// because a single `Connection` is never used to run two transactions
/// concurrently within this codebase's call patterns.
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
        provenance,
        confidence,
        last_used_at,
    } = item;

    let provenance_str = provenance.unwrap_or(Provenance::Observed).as_str();
    let conf_val = confidence.unwrap_or(1.0);

    let tx = conn.unchecked_transaction()?;

    // Archive the previous body as an immutable revision node before replacing.
    if !body.is_empty() {
        if let Some(prev) = mem_get(&tx, project_key, slug)? {
            if !prev.body.is_empty() && prev.body != body {
                let node_id = mem_node_add(
                    &tx,
                    project_key,
                    "revision",
                    &prev.updated_at,
                    &prev.body,
                    prev.session_id.as_deref(),
                )?;
                mem_lineage_add(&tx, "item", &prev.id, "node", &node_id, "revision")?;
            }
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let last_used_str = last_used_at.unwrap_or(&now);

    tx.execute(
        "INSERT INTO mem_items
             (id, project_key, item_type, slug, title, description, body, created_at, updated_at, session_id, layer, provenance, confidence, last_used_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8,?9,?10,?11,?12,?13)
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
             layer       = excluded.layer,
             provenance  = excluded.provenance,
             confidence  = excluded.confidence,
             last_used_at = excluded.last_used_at",
        params![
            id,
            project_key,
            item_type,
            slug,
            title,
            description,
            body,
            now,
            session_id,
            layer,
            provenance_str,
            conf_val,
            last_used_str
        ],
    )?;

    tx.commit()?;
    Ok(())
}

pub fn mem_list(conn: &Connection, project_key: &str) -> Result<Vec<MemItemRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_key, item_type, slug, title, description, body, created_at, updated_at, session_id, layer, provenance, confidence, last_used_at
         FROM mem_items WHERE project_key = ?1 ORDER BY layer DESC, item_type, slug",
    )?;
    let rows = stmt
        .query_map(params![project_key], |row| {
            let prov_str: String = row.get(11)?;
            let provenance = prov_str.parse().unwrap_or(Provenance::Observed);
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
                provenance,
                confidence: row.get(12)?,
                last_used_at: row.get(13)?,
            })
        })?
        .flatten()
        .collect();
    Ok(rows)
}

pub fn mem_get(conn: &Connection, project_key: &str, slug: &str) -> Result<Option<MemItemRow>> {
    conn.query_row(
        "SELECT id, project_key, item_type, slug, title, description, body, created_at, updated_at, session_id, layer, provenance, confidence, last_used_at
         FROM mem_items WHERE project_key = ?1 AND slug = ?2",
        params![project_key, slug],
        |row| {
            let prov_str: String = row.get(11)?;
            let provenance = prov_str.parse().unwrap_or(Provenance::Observed);
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
                provenance,
                confidence: row.get(12)?,
                last_used_at: row.get(13)?,
            })
        },
    )
    .optional()
}

/// Mark a mem_item as used: updates `last_used_at` to current time and boosts `confidence` towards 1.0.
pub fn mem_on_used(conn: &Connection, project_key: &str, slug: &str) -> Result<bool> {
    let now = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let n = conn.execute(
        "UPDATE mem_items
         SET last_used_at = ?1,
             confidence = MIN(1.0, confidence + 0.1)
         WHERE project_key = ?2 AND slug = ?3",
        params![now, project_key, slug],
    )?;
    Ok(n > 0)
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
///
/// `l0_raw` nodes are content-addressed within a project: re-adding the same
/// `(project_key, content)` returns the existing node's id instead of minting
/// a duplicate row. This matters because periodic background sync re-scans
/// the whole session transcript on every tick (see `auto_sync_agent_memory`),
/// so the same evidence line is offered repeatedly — without dedup here, L0
/// evidence (and its lineage edges) would grow unboundedly across a session
/// even though the L1 facts derived from it are already deduped via slug.
///
/// `revision` nodes are NOT deduped: each one is a genuinely distinct
/// historical snapshot of a mem_item's body and must always be inserted
/// fresh, even if its content happens to match an earlier revision's.
///
/// Returns the generated (or, for a deduped l0_raw hit, the existing) node id.
pub fn mem_node_add(
    conn: &Connection,
    project_key: &str,
    kind: &str,
    source: &str,
    content: &str,
    session_id: Option<&str>,
) -> Result<String> {
    if kind == "l0_raw" {
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM mem_nodes WHERE project_key = ?1 AND kind = 'l0_raw' AND content = ?2",
                params![project_key, content],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
    }

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
