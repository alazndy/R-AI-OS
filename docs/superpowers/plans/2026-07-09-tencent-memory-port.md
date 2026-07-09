# Tencent-Style Layered Memory Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port three TencentDB-Agent-Memory concepts natively into raios: (1) lineage refs that fix the `mem_items` append-only body bloat, (2) L0→L3 layered memory distillation, (3) symbolic Mermaid compression over `session_events`.

**Architecture:** All state stays in the single SQLite `workspace.db`. Two new tables (`mem_nodes` for immutable L0/revision evidence, `mem_lineage` for derived-from edges) plus a `layer` column on `mem_items`. `mem_upsert` switches from body-concatenation to replace-and-archive. `session_memory.rs` extraction becomes per-fact atomic (L1) with deterministic hash slugs, then assembles L2 scene blocks and an L3 persona — all template-based, no LLM. A new `session_canvas.rs` folds `session_events` into a Mermaid flowchart with `se:<id>` result refs.

**Tech Stack:** Rust, rusqlite, uuid, chrono, serde (all already in the workspace — no new dependencies).

## Global Constraints

- Single SQLite file: `~/.config/raios/workspace.db` — no new storage files, no second DB.
- Single binary, zero new external crates. Only `rusqlite`, `uuid`, `chrono`, `serde`, `serde_json` (already in `Cargo.toml`).
- Fully local: no LLM calls anywhere in this plan. All distillation is deterministic template/heuristic code.
- Schema changes follow the existing idempotent pattern in `crates/raios-core/src/db/schema.rs`: `let _ = conn.execute_batch("ALTER TABLE ...")` for old DBs (errors silently ignored) + column present in the fresh `CREATE TABLE`.
- Never write to `instinct_candidates` or touch the two-schema instinct tables (Constitution §8.1 rule 5).
- Layer semantics (Tencent pyramid mapped to raios): **L0** = raw transcript line (immutable, `mem_nodes.kind='l0_raw'`), **L1** = atomic fact (`mem_items.layer=1`), **L2** = scene block / per-day digest (`mem_items.layer=2`), **L3** = persona (`mem_items.layer=3`).
- Existing `mem_items` rows are day-aggregates → migrate to `layer=2` by default.
- Commit messages: English, conventional-commit style. Run tests before every commit.
- Repo root: `/home/alaz/dev/core/R-AI-OS`. All commands run from there.

---

### Task 1: Schema — `mem_nodes`, `mem_lineage`, `mem_items.layer`

**Files:**
- Modify: `crates/raios-core/src/db/schema.rs` (ALTER block at top of `migrate()`, new `execute_batch` after the `mem_items` batch at ~line 419-437)
- Create: `crates/raios-core/src/db/tests/mem.rs`
- Modify: `crates/raios-core/src/db/tests/mod.rs` (register `mod mem;`)

**Interfaces:**
- Consumes: existing `migrate()` / `in_memory()` test helper (`crates/raios-core/src/db/tests/mod.rs`).
- Produces: tables `mem_nodes(id, project_key, kind, source, content, session_id, created_at)`, `mem_lineage(id, child_kind, child_id, parent_kind, parent_id, relation, created_at)`, column `mem_items.layer INTEGER NOT NULL DEFAULT 2`. Later tasks rely on these exact names.

- [ ] **Step 1: Write the failing test**

Create `crates/raios-core/src/db/tests/mem.rs`:

```rust
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
```

Register in `crates/raios-core/src/db/tests/mod.rs` — add after `mod integration;`:

```rust
mod mem;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-core mem_schema_has_layer_nodes_lineage`
Expected: FAIL with `no such column: layer` (or `no such table: mem_nodes`).

- [ ] **Step 3: Add schema migration**

In `crates/raios-core/src/db/schema.rs`, inside `migrate()`, add to the idempotent ALTER block at the top (after the `swarm_tasks` ALTER batch, ~line 33):

```rust
    let _ = conn.execute_batch(
        "ALTER TABLE mem_items ADD COLUMN layer INTEGER NOT NULL DEFAULT 2",
    );
```

In the `mem_items` `CREATE TABLE` batch (~line 421), add the column after `session_id  TEXT,`:

```sql
            session_id  TEXT,
            layer       INTEGER NOT NULL DEFAULT 2,
            UNIQUE(project_key, slug)
```

Then add a new `execute_batch` immediately after the `mem_items` batch, before `Ok(())`:

```rust
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS mem_nodes (
            id          TEXT PRIMARY KEY,
            project_key TEXT NOT NULL,
            kind        TEXT NOT NULL CHECK(kind IN ('l0_raw','revision')),
            source      TEXT NOT NULL DEFAULT '',
            content     TEXT NOT NULL,
            session_id  TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now','utc'))
        );
        CREATE INDEX IF NOT EXISTS idx_mem_nodes_project ON mem_nodes(project_key, kind);

        CREATE TABLE IF NOT EXISTS mem_lineage (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            child_kind  TEXT NOT NULL CHECK(child_kind IN ('item','node')),
            child_id    TEXT NOT NULL,
            parent_kind TEXT NOT NULL CHECK(parent_kind IN ('item','node')),
            parent_id   TEXT NOT NULL,
            relation    TEXT NOT NULL DEFAULT 'derived_from',
            created_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
            UNIQUE(child_kind, child_id, parent_kind, parent_id, relation)
        );
        CREATE INDEX IF NOT EXISTS idx_mem_lineage_child  ON mem_lineage(child_kind, child_id);
        CREATE INDEX IF NOT EXISTS idx_mem_lineage_parent ON mem_lineage(parent_kind, parent_id);
        ",
    )?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-core mem_schema_has_layer_nodes_lineage`
Expected: PASS

- [ ] **Step 5: Run the full raios-core suite to catch regressions**

Run: `cargo test -p raios-core`
Expected: all tests PASS (existing `mem_items` inserts unaffected — new column has a default).

- [ ] **Step 6: Commit**

```bash
git add crates/raios-core/src/db/schema.rs crates/raios-core/src/db/tests/mem.rs crates/raios-core/src/db/tests/mod.rs
git commit -m "feat(db): add mem_nodes, mem_lineage tables and mem_items.layer column"
```

---

### Task 2: DB layer — node + lineage CRUD

**Files:**
- Modify: `crates/raios-core/src/db/mem.rs` (append new section at end of file)
- Modify: `crates/raios-core/src/db/tests/mem.rs`

**Interfaces:**
- Consumes: tables from Task 1.
- Produces (later tasks call these exactly as written):
  - `pub struct MemNodeRow { pub id: String, pub project_key: String, pub kind: String, pub source: String, pub content: String, pub session_id: Option<String>, pub created_at: String }`
  - `pub fn mem_node_add(conn: &Connection, project_key: &str, kind: &str, source: &str, content: &str, session_id: Option<&str>) -> Result<String>` — returns the new node id (uuid v4).
  - `pub fn mem_lineage_add(conn: &Connection, child_kind: &str, child_id: &str, parent_kind: &str, parent_id: &str, relation: &str) -> Result<()>` — idempotent (INSERT OR IGNORE).
  - `pub fn mem_lineage_parents(conn: &Connection, child_kind: &str, child_id: &str) -> Result<Vec<(String, String, String)>>` — `(parent_kind, parent_id, relation)` tuples.
  - `pub fn mem_history(conn: &Connection, project_key: &str, slug: &str) -> Result<Vec<MemNodeRow>>` — revision nodes for an item, newest first.

- [ ] **Step 1: Write the failing tests**

Append to `crates/raios-core/src/db/tests/mem.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-core mem_node_add_and_lineage_round_trip mem_history_returns_revision_nodes_newest_first`
Expected: FAIL to compile with `cannot find function mem_node_add`.

- [ ] **Step 3: Implement the DB functions**

Append to `crates/raios-core/src/db/mem.rs`:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p raios-core mem_node mem_history`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add crates/raios-core/src/db/mem.rs crates/raios-core/src/db/tests/mem.rs
git commit -m "feat(db): mem_nodes/mem_lineage CRUD and mem_history query"
```

---

### Task 3: Fix `mem_upsert` bloat — replace body, archive old as revision node

**Files:**
- Modify: `crates/raios-core/src/db/mem.rs` (`MemItemRow`, `MemUpsert`, `mem_upsert`, `mem_list`, `mem_get`)
- Modify: `crates/raios-surface-cli/src/cli/mem.rs:64` (Add handler — new `layer` field)
- Modify: `crates/raios-runtime/src/session_memory.rs:561-573` (auto_sync caller — new `layer` field)
- Modify: `crates/raios-core/src/db/tests/mem.rs`

**Interfaces:**
- Consumes: `mem_node_add`, `mem_lineage_add` from Task 2.
- Produces: `MemUpsert` gains `pub layer: i64`; `MemItemRow` gains `pub layer: i64`. New `mem_upsert` semantics: on slug conflict with a different non-empty body, the OLD body is archived as a `mem_nodes` row (`kind='revision'`, `source` = old `updated_at`) linked via `mem_lineage(child=item, parent=node, relation='revision')`, then the body is REPLACED. Empty incoming body preserves the existing body. All later tasks use these semantics.

- [ ] **Step 1: Write the failing tests**

Append to `crates/raios-core/src/db/tests/mem.rs`:

```rust
#[test]
fn mem_upsert_replaces_body_and_archives_revision() {
    let conn = in_memory();
    let key = "-home-alaz-p";
    let up = |body: &str| MemUpsert {
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-core mem_upsert_replaces mem_upsert_identical`
Expected: FAIL to compile — `MemUpsert` has no field `layer`.

- [ ] **Step 3: Implement**

In `crates/raios-core/src/db/mem.rs`:

Add `pub layer: i64,` to `MemItemRow` (after `session_id`) and `pub layer: i64,` to `MemUpsert` (after `session_id`).

Replace the whole `mem_upsert` function with:

```rust
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
```

Update `mem_list` and `mem_get` SELECTs to include `layer` (append `, layer` to the column list and `layer: row.get(10)?,` to the row mapping in both functions).

- [ ] **Step 4: Fix the two callers**

`crates/raios-surface-cli/src/cli/mem.rs` — in the `MemAction::Add` arm's `MemUpsert { ... }` literal, add:

```rust
                layer: 1,
```

`crates/raios-runtime/src/session_memory.rs` (~line 563) — in the `MemUpsert { ... }` literal inside `auto_sync_agent_memory`, add:

```rust
                layer: 2,
```

(Existing heuristic items are day-aggregates = L2. Task 5 replaces this call site with per-fact L1 writes.)

- [ ] **Step 5: Run the full test suite and check the workspace compiles**

Run: `cargo test -p raios-core && cargo check --workspace`
Expected: all PASS, no compile errors anywhere.

- [ ] **Step 6: Commit**

```bash
git add crates/raios-core/src/db/mem.rs crates/raios-core/src/db/tests/mem.rs crates/raios-surface-cli/src/cli/mem.rs crates/raios-runtime/src/session_memory.rs
git commit -m "fix(db): mem_upsert replaces body and archives revisions via lineage"
```

---

### Task 4: CLI — `raios mem history <slug>` + `--layer` filter on list

**Files:**
- Modify: `crates/raios-surface-cli/src/cli/action_types.rs` (`MemAction` enum, ~line 83)
- Modify: `crates/raios-surface-cli/src/cli/mem.rs` (`cmd_mem` match)

**Interfaces:**
- Consumes: `mem_history`, `MemItemRow.layer` from Tasks 2-3.
- Produces: `raios mem history <slug> [-p <project>]` and `raios mem list --layer <n>`. No programmatic consumers — terminal output only.

- [ ] **Step 1: Add the CLI variants**

In `crates/raios-surface-cli/src/cli/action_types.rs`, inside `pub enum MemAction`:

Add to the `List` variant (after `item_type`):

```rust
        /// Filter by layer: 1=fact, 2=scene, 3=persona
        #[arg(short = 'l', long)]
        layer: Option<i64>,
```

Add a new variant after `Get`:

```rust
    /// Show archived body revisions of a memory item (newest first)
    History {
        slug: String,
        #[arg(short, long)]
        project: Option<String>,
    },
```

- [ ] **Step 2: Implement the handlers**

In `crates/raios-surface-cli/src/cli/mem.rs`:

In the `MemAction::List` arm, destructure the new field (`MemAction::List { project, item_type, layer }`) and add after the `item_type` filter:

```rust
            let items: Vec<_> = if let Some(l) = layer {
                items.into_iter().filter(|i| i.layer == l).collect()
            } else {
                items
            };
```

Also change the per-item print line to show the layer:

```rust
                println!("  [L{}][{:<10}] {}  \x1b[90m{}\x1b[0m", i.layer, i.item_type, i.slug, i.description);
```

Add a new match arm after `MemAction::Get { .. }`:

```rust
        MemAction::History { slug, project } => {
            let key = project_key_for(&project);
            match raios_core::db::mem_history(&conn, &key, &slug) {
                Ok(revs) if revs.is_empty() => println!("  No revisions for {}", slug),
                Ok(revs) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&revs).unwrap_or_default());
                        return;
                    }
                    println!("\n  REVISIONS  {}/{}  ({})\n", key, slug, revs.len());
                    for r in &revs {
                        println!("  \x1b[90m{}\x1b[0m  node:{}\n{}\n", r.created_at, &r.id[..8], r.content);
                    }
                }
                Err(e) => eprintln!("  Error: {e}"),
            }
        }
```

- [ ] **Step 3: Verify it compiles and behaves**

Run: `cargo build -p raios-surface-cli && ./target/debug/raios mem list --layer 2 2>/dev/null | head -5`
Expected: compiles; list output shows `[L2]` prefixes (or "No memory items" if DB empty for cwd).

Run: `./target/debug/raios mem history nonexistent-slug`
Expected: `No revisions for nonexistent-slug`.

- [ ] **Step 4: Commit**

```bash
git add crates/raios-surface-cli/src/cli/action_types.rs crates/raios-surface-cli/src/cli/mem.rs
git commit -m "feat(cli): raios mem history and --layer filter"
```

---

### Task 5: L1 atomic fact extraction with L0 evidence nodes

**Files:**
- Modify: `crates/raios-runtime/src/session_memory.rs` (heuristic section ~lines 310-420 and `auto_sync_agent_memory` ~lines 538-590)

**Interfaces:**
- Consumes: `mem_upsert` (Task 3 semantics), `mem_node_add`, `mem_lineage_add`, `mem_get` (Task 2).
- Produces:
  - `pub(crate) struct AtomicFact { pub item_type: &'static str, pub text: String, pub raw_line: String }`
  - `fn heuristic_extract_facts(transcript: &str) -> Vec<AtomicFact>` — one fact per matched transcript line.
  - `fn fact_slug(item_type: &str, text: &str) -> String` — deterministic: `"{type}-{:012x}"` from FNV-1a 64 of normalized text (lowercased alphanumeric words joined by `-`), truncated to 48 bits. Re-seen facts map to the same slug → dedup for free.
  - `auto_sync_agent_memory` writes, per fact: one `mem_nodes` L0 row (full raw line) + one `mem_items` L1 row + one `derived_from` edge. Task 6/7 build on the L1 rows it creates.
  - Existing `pub fn decision_lines_from_transcript` keeps its exact signature and behavior (returns `Vec<String>` of `- <text>` lines for project-type facts).

- [ ] **Step 1: Write the failing tests**

`session_memory.rs` has no `#[cfg(test)]` module yet — append one at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const TRANSCRIPT: &str = "User: don't use npm here, use pnpm\n\nAssistant: Anlaşıldı.\n\nUser: we decided to use SQLite for everything\n\nUser: ben gömülü sistem geliştiriciyim";

    #[test]
    fn extract_facts_one_per_matched_line() {
        let facts = heuristic_extract_facts(TRANSCRIPT);
        let types: Vec<&str> = facts.iter().map(|f| f.item_type).collect();
        assert!(types.contains(&"feedback"));
        assert!(types.contains(&"project"));
        assert!(types.contains(&"user"));
        // raw_line preserves the untruncated source line
        assert!(facts.iter().any(|f| f.raw_line.contains("don't use npm")));
    }

    #[test]
    fn fact_slug_is_deterministic_and_normalized() {
        let a = fact_slug("feedback", "Don't use NPM here!");
        let b = fact_slug("feedback", "don't use npm  here");
        assert_eq!(a, b);
        assert!(a.starts_with("feedback-"));
        let c = fact_slug("feedback", "something else entirely");
        assert_ne!(a, c);
    }

    #[test]
    fn decision_lines_still_work() {
        let lines = decision_lines_from_transcript(TRANSCRIPT);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("- "));
        assert!(lines[0].contains("SQLite"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-runtime session_memory`
Expected: FAIL to compile — `heuristic_extract_facts` / `fact_slug` not found.

- [ ] **Step 3: Implement fact extraction**

In `crates/raios-runtime/src/session_memory.rs`, in the auto-sync section (after `first_n_words`, ~line 323), add:

```rust
pub(crate) struct AtomicFact {
    pub item_type: &'static str,
    pub text: String,
    pub raw_line: String,
}

fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 14_695_981_039_346_656_037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1_099_511_628_211);
    }
    h
}

fn normalize_fact(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

fn fact_slug(item_type: &str, text: &str) -> String {
    let h = fnv1a64(&normalize_fact(text)) & 0xFFFF_FFFF_FFFF; // 48 bits is plenty
    format!("{}-{:012x}", item_type, h)
}

fn heuristic_extract_facts(transcript: &str) -> Vec<AtomicFact> {
    let mut facts: Vec<AtomicFact> = Vec::new();

    for line in transcript.lines() {
        let Some(text) = line.strip_prefix("User: ") else {
            continue;
        };
        let lower = text.to_lowercase();

        // Feedback — user corrects or confirms a non-obvious approach (EN + TR)
        if ["don't ", "do not ", "stop ", "avoid ", "no, ", "wrong", "not that", "incorrect", "please don't",
            "yapma", "etme", "hayır", "yanlış", "olmaz", "değil", "bunu yapma", "böyle değil",
            "istemiyorum", "kullanma", "ekleme", "silme"]
            .iter()
            .any(|p| lower.contains(p))
        {
            facts.push(AtomicFact {
                item_type: "feedback",
                text: first_n_words(text, 30),
                raw_line: line.to_string(),
            });
        }

        // Project decisions / architecture choices (EN + TR)
        if ["we'll use", "we're using", "we decided", "let's use", "going with", "we chose", "architecture is", "we're building",
            "kullanalım", "kullanıyoruz", "karar verdik", "yapacağız", "tercih", "mimari", "gideceğiz",
            "yapıyoruz", "seçtik", "geçiyoruz", "kullanacağız", "artık", "bundan sonra"]
            .iter()
            .any(|p| lower.contains(p))
        {
            facts.push(AtomicFact {
                item_type: "project",
                text: first_n_words(text, 30),
                raw_line: line.to_string(),
            });
        }

        // User background (EN + TR)
        if ["i'm a ", "i am a ", "i work ", "i've been", "my role", "my stack", "my background", "i specialize",
            "ben ", "benim ", "çalışıyorum", "uzmanlık", "stack'im", "yıldır", "geliştiriciyim", "mühendisim"]
            .iter()
            .any(|p| lower.contains(p))
        {
            facts.push(AtomicFact {
                item_type: "user",
                text: first_n_words(text, 40),
                raw_line: line.to_string(),
            });
        }
    }

    facts
}
```

Rewrite `decision_lines_from_transcript` on top of facts (delete its old body and the now-unused `heuristic_extract` + `HeuristicItem` struct + `to_slug` helper if nothing else references them — verify with `grep -n "heuristic_extract\b\|HeuristicItem\|to_slug" crates/raios-runtime/src/session_memory.rs` and remove only if the fact-pipeline replaced every use):

```rust
pub fn decision_lines_from_transcript(transcript: &str) -> Vec<String> {
    heuristic_extract_facts(transcript)
        .into_iter()
        .filter(|f| f.item_type == "project")
        .map(|f| format!("- {}", f.text))
        .collect()
}
```

- [ ] **Step 4: Rewrite `auto_sync_agent_memory` body**

Replace the section between `let transcript = ...` and the `mem_export` call with:

```rust
    let transcript = collect_transcript(agent, project_path, session_started);
    if transcript.is_empty() {
        return;
    }

    let facts = heuristic_extract_facts(&transcript);
    if facts.is_empty() {
        return;
    }

    let project_key = claude_project_dir_name(project_path);
    let Ok(conn) = raios_core::db::open_db() else { return };

    let mut written = 0usize;
    for fact in &facts {
        // L0: immutable raw evidence
        let Ok(node_id) = raios_core::db::mem_node_add(
            &conn, &project_key, "l0_raw", agent, &fact.raw_line, None,
        ) else {
            continue;
        };

        // L1: atomic fact — hash slug makes re-detection idempotent
        let slug = fact_slug(fact.item_type, &fact.text);
        let title = first_n_words(&fact.text, 8);
        let ok = raios_core::db::mem_upsert(
            &conn,
            raios_core::db::MemUpsert {
                project_key: &project_key,
                item_type: fact.item_type,
                slug: &slug,
                title: &title,
                description: &fact.text,
                body: &fact.text,
                session_id: None,
                layer: 1,
            },
        )
        .is_ok();

        // Lineage: fact derived_from raw line
        if ok {
            if let Ok(Some(item)) = raios_core::db::mem_get(&conn, &project_key, &slug) {
                let _ = raios_core::db::mem_lineage_add(
                    &conn, "item", &item.id, "node", &node_id, "derived_from",
                );
                written += 1;
            }
        }
    }
    let _ = written; // used by Task 6/7 additions
```

Keep the existing `mem_export` block at the end unchanged.

- [ ] **Step 5: Run tests and check compile**

Run: `cargo test -p raios-runtime session_memory && cargo check --workspace`
Expected: 3 tests PASS, workspace compiles.

- [ ] **Step 6: Commit**

```bash
git add crates/raios-runtime/src/session_memory.rs
git commit -m "feat(memory): L1 atomic fact extraction with L0 evidence lineage"
```

---

### Task 6: L2 scene blocks — per-day digest with fact refs

**Files:**
- Modify: `crates/raios-runtime/src/session_memory.rs`

**Interfaces:**
- Consumes: fact pipeline from Task 5; `mem_upsert`, `mem_get`, `mem_lineage_add`.
- Produces: `fn upsert_scene_block(conn: &rusqlite::Connection, project_key: &str, fact_slugs: &[(String, &'static str, String)]) -> Option<String>` — arg tuples are `(slug, item_type, text)`; upserts item `scene-YYYYMMDD` (`layer=2`, `item_type="project"`), links `scene derived_from each fact` (item→item edges), returns the scene slug. Called from `auto_sync_agent_memory`.

- [ ] **Step 1: Write the failing test**

Append inside the `mod tests` block of `session_memory.rs`:

```rust
    #[test]
    fn scene_block_upsert_links_facts() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let key = "-home-alaz-p";

        // seed two L1 facts
        for (t, txt) in [("feedback", "don't use npm"), ("project", "we decided sqlite")] {
            let slug = fact_slug(t, txt);
            raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
                project_key: key, item_type: t, slug: &slug, title: txt,
                description: txt, body: txt, session_id: None, layer: 1,
            }).unwrap();
        }
        let slugs: Vec<(String, &'static str, String)> = vec![
            (fact_slug("feedback", "don't use npm"), "feedback", "don't use npm".into()),
            (fact_slug("project", "we decided sqlite"), "project", "we decided sqlite".into()),
        ];

        let scene_slug = upsert_scene_block(&conn, key, &slugs).unwrap();
        let scene = raios_core::db::mem_get(&conn, key, &scene_slug).unwrap().unwrap();
        assert_eq!(scene.layer, 2);
        assert!(scene.body.contains("don't use npm"));
        assert!(scene.body.contains(&fact_slug("project", "we decided sqlite")));

        // lineage: scene → 2 fact parents
        let parents = raios_core::db::mem_lineage_parents(&conn, "item", &scene.id).unwrap();
        assert_eq!(parents.len(), 2);
        assert!(parents.iter().all(|(k, _, r)| k == "item" && r == "derived_from"));
    }
```

Note: this test needs `raios-core`'s `migrate_existing` — it is already `pub` (`crates/raios-core/src/db/mod.rs:100`) and `raios-runtime` already depends on `raios-core`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime scene_block`
Expected: FAIL to compile — `upsert_scene_block` not found.

- [ ] **Step 3: Implement**

Add to `session_memory.rs` (after `heuristic_extract_facts`):

```rust
/// L2: upsert the per-day scene block digest for today and link it to its facts.
/// Returns the scene slug, or None if fact_slugs is empty or DB writes failed.
fn upsert_scene_block(
    conn: &rusqlite::Connection,
    project_key: &str,
    fact_slugs: &[(String, &'static str, String)],
) -> Option<String> {
    if fact_slugs.is_empty() {
        return None;
    }
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let scene_slug = format!("scene-{}", date.replace('-', ""));

    let body = fact_slugs
        .iter()
        .map(|(slug, t, text)| format!("- [{t}] {text} ([[{slug}]])"))
        .collect::<Vec<_>>()
        .join("\n");

    raios_core::db::mem_upsert(
        conn,
        raios_core::db::MemUpsert {
            project_key,
            item_type: "project",
            slug: &scene_slug,
            title: &format!("Scene ({})", date),
            description: &format!("{} fact(s) distilled", fact_slugs.len()),
            body: &body,
            session_id: None,
            layer: 2,
        },
    )
    .ok()?;

    let scene = raios_core::db::mem_get(conn, project_key, &scene_slug).ok()??;
    for (slug, _, _) in fact_slugs {
        if let Ok(Some(fact)) = raios_core::db::mem_get(conn, project_key, slug) {
            let _ = raios_core::db::mem_lineage_add(
                conn, "item", &scene.id, "item", &fact.id, "derived_from",
            );
        }
    }
    Some(scene_slug)
}
```

Wire it into `auto_sync_agent_memory`: inside the fact loop, collect written slugs, then call the scene upsert before `mem_export`. Change the loop to build the vec (replace the `written` counter):

```rust
    let mut written: Vec<(String, &'static str, String)> = Vec::new();
```

and inside the `if ok { ... }` block, after `mem_lineage_add`:

```rust
                written.push((slug.clone(), fact.item_type, fact.text.clone()));
```

then after the loop (replacing `let _ = written;`):

```rust
    let _ = upsert_scene_block(&conn, &project_key, &written);
```

One rebuild ripple: `upsert_scene_block`'s body-replacement means multiple syncs on the same day regenerate the scene from that sync's facts only. To keep the daily scene cumulative, merge with the existing body — insert this right before the `mem_upsert` call in `upsert_scene_block`:

```rust
    let mut lines: Vec<String> = raios_core::db::mem_get(conn, project_key, &scene_slug)
        .ok()
        .flatten()
        .map(|s| s.body.lines().map(String::from).collect())
        .unwrap_or_default();
    for (slug, t, text) in fact_slugs {
        let line = format!("- [{t}] {text} ([[{slug}]])");
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    let body = lines.join("\n");
```

(and delete the earlier `let body = ...` construction so `body` is defined once, by this merged version).

- [ ] **Step 4: Run tests**

Run: `cargo test -p raios-runtime && cargo check --workspace`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/raios-runtime/src/session_memory.rs
git commit -m "feat(memory): L2 daily scene blocks with fact lineage"
```

---

### Task 7: L3 persona — deterministic rolling profile

**Files:**
- Modify: `crates/raios-runtime/src/session_memory.rs`

**Interfaces:**
- Consumes: L1 items (`layer=1`, types `user`/`feedback`) via `mem_list`; `mem_upsert`, `mem_get`, `mem_lineage_add`.
- Produces: `pub fn rebuild_persona(conn: &rusqlite::Connection, project_key: &str) -> Option<()>` — upserts item slug `persona` (`layer=3`, `item_type="user"`, title `"Persona"`). Body: `## Background` section from latest 10 `user` facts + `## Working Rules` from latest 20 `feedback` facts, each linked `derived_from`. Called at the end of `auto_sync_agent_memory` whenever any user/feedback fact was written this sync.

- [ ] **Step 1: Write the failing test**

Append inside `mod tests`:

```rust
    #[test]
    fn persona_assembles_from_user_and_feedback_facts() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let key = "-home-alaz-p";

        for (t, txt) in [
            ("user", "ben gömülü sistem geliştiriciyim"),
            ("feedback", "don't use npm, use pnpm"),
            ("project", "we decided sqlite"), // must NOT appear in persona
        ] {
            let slug = fact_slug(t, txt);
            raios_core::db::mem_upsert(&conn, raios_core::db::MemUpsert {
                project_key: key, item_type: t, slug: &slug, title: txt,
                description: txt, body: txt, session_id: None, layer: 1,
            }).unwrap();
        }

        rebuild_persona(&conn, key).unwrap();
        let p = raios_core::db::mem_get(&conn, key, "persona").unwrap().unwrap();
        assert_eq!(p.layer, 3);
        assert!(p.body.contains("## Background"));
        assert!(p.body.contains("gömülü sistem"));
        assert!(p.body.contains("## Working Rules"));
        assert!(p.body.contains("use pnpm"));
        assert!(!p.body.contains("sqlite"));

        let parents = raios_core::db::mem_lineage_parents(&conn, "item", &p.id).unwrap();
        assert_eq!(parents.len(), 2);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime persona`
Expected: FAIL to compile — `rebuild_persona` not found.

- [ ] **Step 3: Implement**

Add to `session_memory.rs` (after `upsert_scene_block`):

```rust
/// L3: rebuild the project persona from L1 user/feedback facts. Deterministic, no LLM.
pub fn rebuild_persona(conn: &rusqlite::Connection, project_key: &str) -> Option<()> {
    let items = raios_core::db::mem_list(conn, project_key).ok()?;
    let mut user: Vec<&raios_core::db::MemItemRow> = items
        .iter()
        .filter(|i| i.layer == 1 && i.item_type == "user" && i.slug != "persona")
        .collect();
    let mut feedback: Vec<&raios_core::db::MemItemRow> = items
        .iter()
        .filter(|i| i.layer == 1 && i.item_type == "feedback")
        .collect();
    if user.is_empty() && feedback.is_empty() {
        return None;
    }
    // newest first, capped
    user.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    feedback.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    user.truncate(10);
    feedback.truncate(20);

    let mut body = String::new();
    if !user.is_empty() {
        body.push_str("## Background\n");
        for i in &user {
            body.push_str(&format!("- {} ([[{}]])\n", i.body, i.slug));
        }
    }
    if !feedback.is_empty() {
        body.push_str("\n## Working Rules\n");
        for i in &feedback {
            body.push_str(&format!("- {} ([[{}]])\n", i.body, i.slug));
        }
    }

    raios_core::db::mem_upsert(
        conn,
        raios_core::db::MemUpsert {
            project_key,
            item_type: "user",
            slug: "persona",
            title: "Persona",
            description: &format!(
                "{} background + {} rule fact(s)",
                user.len(),
                feedback.len()
            ),
            body: body.trim_end(),
            session_id: None,
            layer: 3,
        },
    )
    .ok()?;

    let persona = raios_core::db::mem_get(conn, project_key, "persona").ok()??;
    for i in user.iter().chain(feedback.iter()) {
        let _ = raios_core::db::mem_lineage_add(
            conn, "item", &persona.id, "item", &i.id, "derived_from",
        );
    }
    Some(())
}
```

Wire into `auto_sync_agent_memory` — after the `upsert_scene_block` call:

```rust
    if written.iter().any(|(_, t, _)| *t == "user" || *t == "feedback") {
        let _ = rebuild_persona(&conn, &project_key);
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p raios-runtime && cargo check --workspace`
Expected: all PASS. Note: persona body archiving on every rebuild is intentional — revisions are cheap rows, and `mem_history persona` becomes the persona timeline (Tencent's traceability principle).

- [ ] **Step 5: Commit**

```bash
git add crates/raios-runtime/src/session_memory.rs
git commit -m "feat(memory): L3 deterministic persona assembly with lineage"
```

---

### Task 8: Layer-aware export — persona-first MEMORY.md

**Files:**
- Modify: `crates/raios-core/src/db/mem.rs` (`mem_export`, `mem_list` ORDER BY)
- Modify: `crates/raios-core/src/db/tests/mem.rs`

**Interfaces:**
- Consumes: `MemItemRow.layer`.
- Produces: exported markdown frontmatter gains `  layer: N`; `MEMORY.md` lists items grouped by layer, highest first (`## Persona (L3)`, `## Scenes (L2)`, `## Facts (L1)`). `mem_list` orders by `layer DESC, item_type, slug`.

- [ ] **Step 1: Write the failing test**

Append to `crates/raios-core/src/db/tests/mem.rs`:

```rust
#[test]
fn mem_export_groups_by_layer_persona_first() {
    let conn = in_memory();
    let key = "-home-alaz-p";
    for (slug, layer, t) in [("persona", 3, "user"), ("scene-20260709", 2, "project"), ("feedback-abc", 1, "feedback")] {
        mem_upsert(&conn, MemUpsert {
            project_key: key, item_type: t, slug, title: slug,
            description: "d", body: "b", session_id: None, layer,
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-core mem_export_groups`
Expected: FAIL — frontmatter has no `layer:`, index has no layer headings.

- [ ] **Step 3: Implement**

In `mem_list`, change the ORDER BY to:

```sql
         FROM mem_items WHERE project_key = ?1 ORDER BY layer DESC, item_type, slug
```

In `mem_export`, change the per-file `content` format to include layer:

```rust
        let content = format!(
            "---\nname: {}\ndescription: {}\nmetadata:\n  type: {}\n  layer: {}\n---\n\n{}\n",
            item.slug, item.description, item.item_type, item.layer, item.body
        );
```

Replace the `entries` construction with layer-grouped sections:

```rust
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
```

(The old `entries`/`content` lines are removed; `header` logic stays.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p raios-core && cargo check --workspace`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/raios-core/src/db/mem.rs crates/raios-core/src/db/tests/mem.rs
git commit -m "feat(memory): layer-aware export, persona-first MEMORY.md index"
```

---

### Task 9: Symbolic canvas — Mermaid compression over session_events

**Files:**
- Create: `crates/raios-runtime/src/session_canvas.rs`
- Modify: `crates/raios-runtime/src/lib.rs` (register `pub mod session_canvas;` next to the existing `pub mod session;` line — locate with `grep -n "pub mod session" crates/raios-runtime/src/lib.rs`)
- Modify: `crates/raios-surface-cli/src/cli/args.rs:327-333` (`Sessions` variant)
- Modify: `crates/raios-surface-cli/src/cli/session.rs` (`cmd_sessions`)
- Modify: dispatch arm — locate with `grep -n "cmd_sessions" crates/raios-surface-cli/src/cli/mod.rs` and pass the new arg through.

**Interfaces:**
- Consumes: `raios_runtime::session::{SessionStore, SessionEvent}` (`SessionEvent { id: i64, session_id: String, event_type: String, data: String, timestamp: String }`; `SessionStore::default_path()`, `SessionStore::new(path)`, `store.events(session_id) -> Vec<SessionEvent>`).
- Produces:
  - `pub struct CanvasNode { pub label: String, pub count: usize, pub first_ref: i64, pub detail: Option<String> }`
  - `pub fn fold_events(events: &[SessionEvent]) -> Vec<CanvasNode>` — collapses consecutive runs of the same `event_type` into one node with a count; `data` longer than 60 chars is truncated into `detail` with the full payload left addressable in the DB via `se:<first_ref>` (Tencent's `result_ref` principle: compression is never irreversible).
  - `pub fn to_mermaid(session_id: &str, nodes: &[CanvasNode]) -> String` — `flowchart TD` text.
  - CLI: `raios sessions --canvas <session_id>` prints the Mermaid canvas.

- [ ] **Step 1: Write the failing tests**

Create `crates/raios-runtime/src/session_canvas.rs`:

```rust
//! Session Canvas — symbolic short-term compression over session_events.
//!
//! Folds a session's event stream into a compact Mermaid flowchart. Long
//! payloads are truncated in the label but stay addressable in the DB via
//! `se:<event_id>` refs, so no compression step is irreversible.

use crate::session::SessionEvent;

pub struct CanvasNode {
    pub label: String,
    pub count: usize,
    pub first_ref: i64,
    pub detail: Option<String>,
}

const DETAIL_MAX: usize = 60;

/// Collapse consecutive runs of the same event_type into single nodes.
pub fn fold_events(events: &[SessionEvent]) -> Vec<CanvasNode> {
    let mut nodes: Vec<CanvasNode> = Vec::new();
    for ev in events {
        match nodes.last_mut() {
            Some(last) if last.label == ev.event_type => {
                last.count += 1;
            }
            _ => {
                let detail = if ev.data.is_empty() {
                    None
                } else {
                    let chars: Vec<char> = ev.data.chars().collect();
                    if chars.len() > DETAIL_MAX {
                        Some(format!("{}…", chars[..DETAIL_MAX].iter().collect::<String>()))
                    } else {
                        Some(ev.data.clone())
                    }
                };
                nodes.push(CanvasNode {
                    label: ev.event_type.clone(),
                    count: 1,
                    first_ref: ev.id,
                    detail,
                });
            }
        }
    }
    nodes
}

/// Render folded nodes as a Mermaid flowchart.
pub fn to_mermaid(session_id: &str, nodes: &[CanvasNode]) -> String {
    let mut out = String::from("flowchart TD\n");
    let short_id: String = session_id.chars().take(8).collect();
    out.push_str(&format!("    S((session {short_id}))\n"));
    let mut prev = "S".to_string();
    for (i, n) in nodes.iter().enumerate() {
        let id = format!("N{i}");
        let count = if n.count > 1 {
            format!(" ×{}", n.count)
        } else {
            String::new()
        };
        let detail = n
            .detail
            .as_deref()
            .map(|d| format!(": {}", d.replace('"', "'")))
            .unwrap_or_default();
        out.push_str(&format!(
            "    {prev} --> {id}[\"{}{count}{detail} (se:{})\"]\n",
            n.label, n.first_ref
        ));
        prev = id;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: i64, t: &str, data: &str) -> SessionEvent {
        SessionEvent {
            id,
            session_id: "s1".into(),
            event_type: t.into(),
            data: data.into(),
            timestamp: "2026-07-09 12:00:00".into(),
        }
    }

    #[test]
    fn fold_collapses_consecutive_runs() {
        let events = vec![
            ev(1, "file_read", "a.rs"),
            ev(2, "file_read", "b.rs"),
            ev(3, "tool_call", "cargo test"),
            ev(4, "file_read", "c.rs"),
        ];
        let nodes = fold_events(&events);
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].count, 2);
        assert_eq!(nodes[0].first_ref, 1);
        assert_eq!(nodes[1].label, "tool_call");
        assert_eq!(nodes[2].count, 1);
    }

    #[test]
    fn fold_truncates_long_payloads_keeping_ref() {
        let long = "x".repeat(200);
        let nodes = fold_events(&[ev(7, "tool_call", &long)]);
        let detail = nodes[0].detail.as_ref().unwrap();
        assert!(detail.chars().count() <= 61); // 60 + ellipsis
        assert!(detail.ends_with('…'));
        assert_eq!(nodes[0].first_ref, 7); // full payload reachable: se:7
    }

    #[test]
    fn mermaid_output_shape() {
        let nodes = fold_events(&[ev(1, "file_read", "a.rs"), ev(2, "file_read", "b.rs")]);
        let m = to_mermaid("620c3156-abcd", &nodes);
        assert!(m.starts_with("flowchart TD"));
        assert!(m.contains("session 620c3156"));
        assert!(m.contains("file_read ×2"));
        assert!(m.contains("(se:1)"));
    }
}
```

Register the module in `crates/raios-runtime/src/lib.rs` next to `pub mod session;`:

```rust
pub mod session_canvas;
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p raios-runtime session_canvas`
Expected: 3 tests PASS (implementation is written with the tests in this file; verify all three actually run).

- [ ] **Step 3: Add the CLI flag**

In `crates/raios-surface-cli/src/cli/args.rs`, add to the `Sessions` variant (after the `top` field):

```rust
        /// Render one session's event stream as a Mermaid canvas
        #[arg(long)]
        canvas: Option<String>,
```

Locate the dispatch: `grep -n "cmd_sessions" crates/raios-surface-cli/src/cli/mod.rs` — extend the `Sessions { .. }` match arm to destructure `canvas` and pass it through as a new first check. In `crates/raios-surface-cli/src/cli/session.rs`, change `cmd_sessions`'s signature to accept it and handle it before the list logic:

```rust
pub(super) fn cmd_sessions(agent: Option<&str>, top: usize, canvas: Option<&str>, json: bool) {
    if let Some(session_id) = canvas {
        let store = raios_runtime::session::SessionStore::new(
            raios_runtime::session::SessionStore::default_path(),
        );
        let events = store.events(session_id);
        if events.is_empty() {
            eprintln!("  No events for session {}", session_id);
            return;
        }
        let nodes = raios_runtime::session_canvas::fold_events(&events);
        println!("{}", raios_runtime::session_canvas::to_mermaid(session_id, &nodes));
        return;
    }
    // ... existing body unchanged
```

Note: if `raios-surface-cli/Cargo.toml` does not yet depend on `raios-runtime` (check with `grep raios-runtime crates/raios-surface-cli/Cargo.toml`), add under `[dependencies]`:

```toml
raios-runtime = { path = "../raios-runtime" }
```

- [ ] **Step 4: Verify end-to-end**

Run: `cargo build -p raios-surface-cli && ./target/debug/raios sessions --canvas $(sqlite3 ~/.config/raios/workspace.db "SELECT id FROM sessions ORDER BY started_at DESC LIMIT 1" 2>/dev/null || echo none)`
Expected: a `flowchart TD` block for the latest session, or `No events for session ...` if the events table is empty. Also run `cargo test --workspace` — all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/raios-runtime/src/session_canvas.rs crates/raios-runtime/src/lib.rs crates/raios-surface-cli/src/cli/args.rs crates/raios-surface-cli/src/cli/session.rs crates/raios-surface-cli/src/cli/mod.rs crates/raios-surface-cli/Cargo.toml
git commit -m "feat(session): symbolic Mermaid canvas over session_events"
```

---

### Task 10: Documentation & workspace compliance

**Files:**
- Modify: `/home/alaz/dev/core/R-AI-OS/memory.md` (Change Log & Agent Trail)
- Modify: `/home/alaz/dev/core/R-AI-OS/docs/WIKI/03-Hybrid-Memory-and-Context.md` (new section)
- Modify: `/home/alaz/AGENT_CONSTITUTION.md` §8.1 rule 3 (now outdated)
- Regenerate: `SIGMAP.md`

**Interfaces:** none — documentation only.

- [ ] **Step 1: Update memory.md Change Log**

Append under `## Change Log & Agent Trail` in `/home/alaz/dev/core/R-AI-OS/memory.md`:

```markdown
- [2026-07-09] [Claude Kaira]: Ported TencentDB-Agent-Memory concepts: mem_nodes/mem_lineage (L0 evidence + revision archive, fixes append-only body bloat), L1 atomic facts w/ hash slugs, L2 daily scenes, L3 deterministic persona, layer-aware export, Mermaid session canvas (raios sessions --canvas).
```

- [ ] **Step 2: Add wiki section**

Append to `docs/WIKI/03-Hybrid-Memory-and-Context.md` before the closing tagline:

```markdown
## 5. Layered Distillation (L0→L3)

Inspired by TencentDB-Agent-Memory's semantic pyramid, `mem_items` now carries a `layer`:

| Layer | What | Where |
|---|---|---|
| L0 | Raw transcript lines (immutable evidence) | `mem_nodes` (`kind='l0_raw'`) |
| L1 | Atomic facts, deterministic hash slugs | `mem_items` (`layer=1`) |
| L2 | Daily scene blocks referencing facts | `mem_items` (`layer=2`, `scene-YYYYMMDD`) |
| L3 | Rolling persona (background + working rules) | `mem_items` (`layer=3`, slug `persona`) |

Every abstraction links down via `mem_lineage` (`derived_from` / `revision` edges) — no
compression step is irreversible. `mem_upsert` replaces bodies and archives the previous
version as a `revision` node; `raios mem history <slug>` walks the chain. Session event
streams compress into Mermaid canvases (`raios sessions --canvas <id>`) with `se:<id>`
back-refs to full payloads.
```

- [ ] **Step 3: Update Constitution §8.1 rule 3**

In `/home/alaz/AGENT_CONSTITUTION.md`, replace rule 3's body text (keep the heading number):

```markdown
3. **`mem_items.body` is replace-on-write with revision archive.** Upserting a slug replaces
   the body; the previous body is archived as a `mem_nodes` revision linked via `mem_lineage`.
   * **Mandatory:** use `raios mem history <slug>` to inspect prior versions instead of
     expecting them inline in the body.
   * **Fallback:** if history is unavailable, the current body alone is the source of truth.
```

- [ ] **Step 4: Regenerate SIGMAP and verify health**

Run: `cd /home/alaz/dev/core/R-AI-OS && sigmap && raios health R-AI-OS`
Expected: `SIGMAP.md` regenerated; health returns clean (or only pre-existing findings).

- [ ] **Step 5: Commit**

```bash
git add memory.md docs/WIKI/03-Hybrid-Memory-and-Context.md SIGMAP.md
git commit -m "docs: layered memory (L0-L3), lineage, and session canvas"
```

(`AGENT_CONSTITUTION.md` lives outside the repo — no commit needed there.)
