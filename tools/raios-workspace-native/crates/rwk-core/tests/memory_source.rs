//! `SqliteMemorySource` against a real SQLite file with the RAIOS memory schema.

use std::fs;
use std::path::PathBuf;

use rusqlite::{params, Connection};
use rwk_core::error::ErrorCode;
use rwk_core::memory::{
    MemorySource, SqliteMemorySource, MAX_MEMORY_BODY_CHARS, MAX_MEMORY_RECORDS,
    MAX_MEMORY_REVISIONS,
};
use rwk_core::project::ProjectRoot;
use tempfile::TempDir;

/// The subset of `workspace.db` the memory view reads, copied from raios-core's schema.
const SCHEMA: &str = "
CREATE TABLE projects (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    path TEXT UNIQUE NOT NULL
);
CREATE TABLE mem_items (
    id          TEXT PRIMARY KEY,
    project_key TEXT NOT NULL,
    item_type   TEXT NOT NULL CHECK(item_type IN ('user','feedback','project','reference')),
    slug        TEXT NOT NULL,
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    body        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
    session_id  TEXT,
    layer       INTEGER NOT NULL DEFAULT 2,
    provenance  TEXT NOT NULL DEFAULT 'observed',
    confidence  REAL NOT NULL DEFAULT 1.0,
    last_used_at TEXT,
    UNIQUE(project_key, slug)
);
CREATE TABLE mem_nodes (
    id          TEXT PRIMARY KEY,
    project_key TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK(kind IN ('l0_raw','revision')),
    source      TEXT NOT NULL DEFAULT '',
    content     TEXT NOT NULL,
    session_id  TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now','utc'))
);
CREATE TABLE mem_lineage (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    child_kind  TEXT NOT NULL CHECK(child_kind IN ('item','node')),
    child_id    TEXT NOT NULL,
    parent_kind TEXT NOT NULL CHECK(parent_kind IN ('item','node')),
    parent_id   TEXT NOT NULL,
    relation    TEXT NOT NULL DEFAULT 'derived_from',
    created_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
    UNIQUE(child_kind, child_id, parent_kind, parent_id, relation)
);
";

struct Fixture {
    _directory: TempDir,
    db_path: PathBuf,
    project: ProjectRoot,
    /// The legacy namespace key of `project`: its root with `/` replaced by `-`.
    key: String,
    writer: Connection,
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let project_dir = directory.path().join("app");
    fs::create_dir(&project_dir).unwrap();
    let db_path = directory.path().join("memory.db");
    let writer = Connection::open(&db_path).unwrap();
    writer.execute_batch(SCHEMA).unwrap();
    let project = ProjectRoot::open(project_dir).unwrap();
    let key = project.info().root.to_string_lossy().replace('/', "-");
    Fixture {
        _directory: directory,
        db_path,
        project,
        key,
        writer,
    }
}

impl Fixture {
    fn source(&self) -> SqliteMemorySource {
        SqliteMemorySource::new(self.db_path.clone())
    }

    fn item(&self, project_key: &str, slug: &str, body: &str, layer: i64) -> String {
        let id = format!("{project_key}:{slug}");
        self.writer
            .execute(
                "INSERT INTO mem_items (id, project_key, item_type, slug, title, description, body, layer)
                 VALUES (?1, ?2, 'project', ?3, ?3, 'Decision', ?4, ?5)",
                params![id, project_key, slug, body, layer],
            )
            .unwrap();
        id
    }

    fn revision(&self, project_key: &str, item_id: &str, content: &str, created_at: &str) {
        let node = format!("node-{item_id}-{created_at}");
        self.writer
            .execute(
                "INSERT INTO mem_nodes (id, project_key, kind, source, content, created_at)
                 VALUES (?1, ?2, 'revision', 'fixture', ?3, ?4)",
                params![node, project_key, content, created_at],
            )
            .unwrap();
        self.writer
            .execute(
                "INSERT INTO mem_lineage (child_kind, child_id, parent_kind, parent_id, relation)
                 VALUES ('item', ?1, 'node', ?2, 'revision')",
                params![item_id, node],
            )
            .unwrap();
    }
}

#[test]
fn a_snapshot_lists_only_this_projects_records_highest_layer_first() {
    let f = fixture();
    f.item(&f.key, "decision", "Current body", 2);
    f.item(&f.key, "persona", "Who I am", 3);
    f.item("-some-other-project", "decision", "Other project", 2);

    let snapshot = f.source().snapshot(&f.project).unwrap();

    assert_eq!(snapshot.project_id, f.project.info().id);
    assert_eq!(snapshot.total, 2);
    let slugs: Vec<&str> = snapshot.items.iter().map(|i| i.slug.as_str()).collect();
    assert_eq!(slugs, ["persona", "decision"]);
    assert_eq!(snapshot.items[1].body, "Current body");
    assert!(!snapshot.items[1].truncated);
}

#[test]
fn history_returns_revisions_newest_first_and_ignores_forged_foreign_links() {
    let f = fixture();
    let item = f.item(&f.key, "decision", "Current body", 2);
    f.revision(&f.key, &item, "Old body", "2026-01-01 00:00:01");
    f.revision(&f.key, &item, "Older body", "2026-01-01 00:00:00");
    // A node of another project linked to this project's item must never be revealed.
    f.revision(
        "-project-b",
        &item,
        "Foreign revision",
        "2026-01-01 00:00:09",
    );

    let history = f.source().history(&f.project, "decision").unwrap();

    assert_eq!(history.slug, "decision");
    assert_eq!(history.total, 2);
    let contents: Vec<&str> = history
        .revisions
        .iter()
        .map(|r| r.content.as_str())
        .collect();
    assert_eq!(contents, ["Old body", "Older body"]);

    let injection = f.source().history(&f.project, "' OR 1=1 --").unwrap();
    assert!(injection.revisions.is_empty());
}

#[test]
fn a_snapshot_is_bounded_and_marks_truncated_records() {
    let f = fixture();
    for index in 0..(MAX_MEMORY_RECORDS + 5) {
        f.item(&f.key, &format!("slug-{index}"), &"ü".repeat(9000), 2);
    }

    let snapshot = f.source().snapshot(&f.project).unwrap();

    assert_eq!(snapshot.total, MAX_MEMORY_RECORDS + 5);
    assert_eq!(snapshot.items.len(), MAX_MEMORY_RECORDS);
    assert!(snapshot
        .items
        .iter()
        .all(|item| item.truncated && item.body.chars().count() == MAX_MEMORY_BODY_CHARS));
}

#[test]
fn history_is_bounded_and_newest_first() {
    let f = fixture();
    let item = f.item(&f.key, "decision", "Current", 2);
    for second in 0..(MAX_MEMORY_REVISIONS + 5) {
        f.revision(
            &f.key,
            &item,
            &format!("revision {second}"),
            &format!("2026-01-01 00:00:{second:02}"),
        );
    }

    let history = f.source().history(&f.project, "decision").unwrap();

    assert_eq!(history.total, MAX_MEMORY_REVISIONS + 5);
    assert_eq!(history.revisions.len(), MAX_MEMORY_REVISIONS);
    assert_eq!(history.revisions[0].content, "revision 54");
}

#[test]
fn history_rejects_empty_and_over_long_slugs() {
    let f = fixture();
    for slug in [String::new(), "x".repeat(513)] {
        let error = f.source().history(&f.project, &slug).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{} chars", slug.len());
    }
}

#[test]
fn registered_projects_with_colliding_namespaces_are_rejected_not_merged() {
    let f = fixture();
    f.item(&f.key, "decision", "Cannot assign ownership", 2);
    let root = f.project.info().root.to_string_lossy().into_owned();
    // Same legacy key, different path: the last `/` becomes `-`.
    let collision = format!(
        "{}-{}",
        &root[..root.rfind('/').unwrap()],
        f.project.info().name
    );
    f.writer
        .execute(
            "INSERT INTO projects (name, path) VALUES ('collision', ?1)",
            [collision],
        )
        .unwrap();

    assert_eq!(
        f.source().snapshot(&f.project).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        f.source().history(&f.project, "decision").unwrap_err().code,
        ErrorCode::Conflict
    );
}

#[test]
fn opening_never_creates_or_migrates_a_database() {
    let f = fixture();
    let missing = f.db_path.with_file_name("does-not-exist.db");

    let error = SqliteMemorySource::new(missing.clone())
        .snapshot(&f.project)
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Io);
    assert!(!missing.exists(), "a missing database must not be created");

    let bare = f.db_path.with_file_name("bare.db");
    Connection::open(&bare)
        .unwrap()
        .execute_batch("CREATE TABLE sentinel(value TEXT)")
        .unwrap();
    assert!(SqliteMemorySource::new(bare.clone())
        .snapshot(&f.project)
        .is_err());
    let tables: i64 = Connection::open(&bare)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 1, "no table may be created");
}

#[test]
fn reading_never_changes_the_database_file() {
    let f = fixture();
    let item = f.item(&f.key, "decision", "Current body", 2);
    f.revision(&f.key, &item, "Old body", "2026-01-01 00:00:01");
    let before = fs::read(&f.db_path).unwrap();

    f.source().snapshot(&f.project).unwrap();
    f.source().history(&f.project, "decision").unwrap();

    assert_eq!(fs::read(&f.db_path).unwrap(), before);
    let (confidence, last_used): (f64, Option<String>) = f
        .writer
        .query_row(
            "SELECT confidence, last_used_at FROM mem_items WHERE id = ?1",
            [&item],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(confidence, 1.0);
    assert_eq!(last_used, None);
}

#[test]
fn the_display_redactor_is_applied_to_every_displayed_text_field() {
    let f = fixture();
    let item = f.item(&f.key, "decision", "token sk-secret-123", 2);
    f.writer
        .execute(
            "UPDATE mem_items SET description = 'uses sk-secret-123', provenance = 'sk-secret-123' WHERE id = ?1",
            [&item],
        )
        .unwrap();
    f.revision(&f.key, &item, "old sk-secret-123", "2026-01-01 00:00:01");
    let source = f
        .source()
        .with_redactor(|text| text.replace("sk-secret-123", "[REDACTED]"));

    let snapshot = source.snapshot(&f.project).unwrap();
    let shown = &snapshot.items[0];
    assert_eq!(shown.body, "token [REDACTED]");
    assert_eq!(shown.description, "uses [REDACTED]");
    assert_eq!(shown.provenance, "[REDACTED]");
    let history = source.history(&f.project, "decision").unwrap();
    assert_eq!(history.revisions[0].content, "old [REDACTED]");

    // Without a redactor the text is shown as stored.
    let plain = f.source().snapshot(&f.project).unwrap();
    assert_eq!(plain.items[0].body, "token sk-secret-123");
}
