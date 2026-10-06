//! Read-only projection of a project's RAIOS memory records (`mem_items`, `mem_nodes`,
//! `mem_lineage`), as shown by the memory view. Never writes, migrates or touches usage and
//! confidence counters.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};

use crate::error::{AppError, ErrorCode, Result};
use crate::project::{ProjectId, ProjectRoot};

/// Most records one snapshot loads.
pub const MAX_MEMORY_RECORDS: usize = 200;
/// Most archived revisions one history loads.
pub const MAX_MEMORY_REVISIONS: usize = 50;
/// Most characters of one record body or revision that are loaded.
pub const MAX_MEMORY_BODY_CHARS: usize = 8_000;

/// Longest slug, title and description the view loads, in characters.
const MAX_IDENTITY_CHARS: usize = 512;
/// How long a query waits for a writer to finish before it gives up.
const BUSY_TIMEOUT: Duration = Duration::from_millis(500);

/// One structured memory record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryItem {
    /// Record identifier.
    pub id: String,
    /// Stable slug within the project namespace.
    pub slug: String,
    /// Display title.
    pub title: String,
    /// One-line description.
    pub description: String,
    /// `user`, `feedback`, `project` or `reference`.
    pub item_type: String,
    /// Memory layer: 1 facts, 2 scenes, 3 persona.
    pub layer: i64,
    /// Where the record came from.
    pub provenance: String,
    /// Last update timestamp as stored.
    pub updated_at: String,
    /// Body, cut at [`MAX_MEMORY_BODY_CHARS`].
    pub body: String,
    /// True when the stored body was longer than what is loaded.
    pub truncated: bool,
}

/// The records loaded for one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySnapshot {
    /// Project the records belong to.
    pub project_id: ProjectId,
    /// Loaded records, at most [`MAX_MEMORY_RECORDS`].
    pub items: Vec<MemoryItem>,
    /// Total records stored, so the UI can show loaded versus total.
    pub total: usize,
}

/// One archived revision of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRevision {
    /// Revision identifier.
    pub id: String,
    /// Creation timestamp as stored.
    pub created_at: String,
    /// Revision body, cut at [`MAX_MEMORY_BODY_CHARS`].
    pub content: String,
    /// True when the stored revision was longer than what is loaded.
    pub truncated: bool,
}

/// The archived revisions of one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryHistory {
    /// Project the record belongs to.
    pub project_id: ProjectId,
    /// Slug of the record.
    pub slug: String,
    /// Loaded revisions, at most [`MAX_MEMORY_REVISIONS`].
    pub revisions: Vec<MemoryRevision>,
    /// Total revisions stored.
    pub total: usize,
}

/// Where memory records come from.
pub trait MemorySource: Send + Sync {
    /// Loads the records for `project`. A registered project whose legacy path key collides
    /// with another registered project is rejected rather than merged.
    fn snapshot(&self, project: &ProjectRoot) -> Result<MemorySnapshot>;

    /// Loads the archived revisions of one record; both ends of every lineage link must belong
    /// to the project's namespace.
    fn history(&self, project: &ProjectRoot, slug: &str) -> Result<MemoryHistory>;
}

type Redactor = Box<dyn Fn(&str) -> String + Send + Sync>;

/// [`MemorySource`] backed by a read-only SQLite connection to `workspace.db`.
pub struct SqliteMemorySource {
    db_path: PathBuf,
    redactor: Redactor,
}

impl SqliteMemorySource {
    /// Creates a source for the database at `db_path`. Nothing is opened until a query runs.
    /// Text is shown as stored; see [`SqliteMemorySource::with_redactor`].
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            db_path,
            redactor: Box::new(|text| text.to_owned()),
        }
    }

    /// Applies `redactor` to every text field before it is returned, so secrets stored in
    /// memory never reach the screen. The application passes the RAIOS secret redactor;
    /// `rwk-core` itself does not depend on `raios-core`.
    pub fn with_redactor(
        mut self,
        redactor: impl Fn(&str) -> String + Send + Sync + 'static,
    ) -> Self {
        self.redactor = Box::new(redactor);
        self
    }

    /// Opens the database read-only. A missing file is an error, never created.
    fn open(&self) -> Result<Connection> {
        let connection =
            Connection::open_with_flags(&self.db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(unavailable)?;
        connection.busy_timeout(BUSY_TIMEOUT).map_err(unavailable)?;
        Ok(connection)
    }

    fn show(&self, text: String) -> String {
        (self.redactor)(&text)
    }
}

impl MemorySource for SqliteMemorySource {
    fn snapshot(&self, project: &ProjectRoot) -> Result<MemorySnapshot> {
        let connection = self.open()?;
        let transaction = connection.unchecked_transaction().map_err(unavailable)?;
        let key = namespace(&transaction, &project.info().root)?;
        let total: i64 = transaction
            .query_row(
                "SELECT count(*) FROM mem_items WHERE project_key = ?1",
                [&key],
                |row| row.get(0),
            )
            .map_err(unavailable)?;
        let sql = format!(
            "SELECT id, slug, substr(title,1,{ident}), substr(description,1,{ident}),
                    substr(item_type,1,64), layer, substr(provenance,1,64), substr(updated_at,1,64),
                    substr(body,1,{body}),
                    length(body)>{body} OR length(title)>{ident} OR length(description)>{ident}
             FROM mem_items
             WHERE project_key = ?1 AND length(slug)<={ident} AND length(id)<=128
             ORDER BY layer DESC, item_type, slug LIMIT {limit}",
            ident = MAX_IDENTITY_CHARS,
            body = MAX_MEMORY_BODY_CHARS,
            limit = MAX_MEMORY_RECORDS,
        );
        let items = {
            let mut statement = transaction.prepare(&sql).map_err(unavailable)?;
            let rows = statement
                .query_map([&key], |row| {
                    Ok(MemoryItem {
                        id: row.get(0)?,
                        slug: row.get(1)?,
                        title: self.show(row.get(2)?),
                        description: self.show(row.get(3)?),
                        item_type: self.show(row.get(4)?),
                        layer: row.get(5)?,
                        provenance: self.show(row.get(6)?),
                        updated_at: row.get(7)?,
                        body: self.show(row.get(8)?),
                        truncated: row.get(9)?,
                    })
                })
                .map_err(unavailable)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(unavailable)?
        };
        transaction.commit().map_err(unavailable)?;
        Ok(MemorySnapshot {
            project_id: project.info().id.clone(),
            items,
            total: usize::try_from(total).unwrap_or(0),
        })
    }

    fn history(&self, project: &ProjectRoot, slug: &str) -> Result<MemoryHistory> {
        if slug.is_empty() || slug.chars().count() > MAX_IDENTITY_CHARS {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Invalid memory identity.",
            ));
        }
        let connection = self.open()?;
        let transaction = connection.unchecked_transaction().map_err(unavailable)?;
        let key = namespace(&transaction, &project.info().root)?;
        // Both ends are project-scoped: a forged lineage edge cannot reveal another project's
        // node. Only these static SQL fragments are composed; project and slug are parameters.
        let from = "FROM mem_items i
                    JOIN mem_lineage l ON l.child_kind='item' AND l.child_id=i.id AND l.relation='revision'
                    JOIN mem_nodes n ON l.parent_kind='node' AND l.parent_id=n.id
                    WHERE i.project_key=?1 AND i.slug=?2 AND n.project_key=?1 AND n.kind='revision'";
        let total: i64 = transaction
            .query_row(
                &format!("SELECT count(*) {from}"),
                params![key, slug],
                |row| row.get(0),
            )
            .map_err(unavailable)?;
        let sql = format!(
            "SELECT n.id, substr(n.created_at,1,64), substr(n.content,1,{body}), length(n.content)>{body}
             {from} ORDER BY n.created_at DESC, l.id DESC LIMIT {limit}",
            body = MAX_MEMORY_BODY_CHARS,
            limit = MAX_MEMORY_REVISIONS,
        );
        let revisions = {
            let mut statement = transaction.prepare(&sql).map_err(unavailable)?;
            let rows = statement
                .query_map(params![key, slug], |row| {
                    Ok(MemoryRevision {
                        id: row.get(0)?,
                        created_at: row.get(1)?,
                        content: self.show(row.get(2)?),
                        truncated: row.get(3)?,
                    })
                })
                .map_err(unavailable)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(unavailable)?
        };
        transaction.commit().map_err(unavailable)?;
        Ok(MemoryHistory {
            project_id: project.info().id.clone(),
            slug: slug.to_owned(),
            revisions,
            total: usize::try_from(total).unwrap_or(0),
        })
    }
}

/// The legacy RAIOS memory namespace of a project root: the path with `/` replaced by `-`.
/// It is not an injective encoding, so a registered project that maps to the same key but has
/// a different path makes the namespace ambiguous and the lookup is refused.
fn namespace(connection: &Connection, root: &Path) -> Result<String> {
    let root = root.to_string_lossy();
    let key = root.replace('/', "-");
    let ambiguous: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE replace(path, '/', '-') = ?1 AND path <> ?2)",
            params![key, root.as_ref()],
            |row| row.get(0),
        )
        .map_err(unavailable)?;
    if ambiguous {
        return Err(AppError::new(
            ErrorCode::Conflict,
            "Registered projects share one memory key; use the file memory instead.",
        ));
    }
    Ok(key)
}

fn unavailable(error: rusqlite::Error) -> AppError {
    AppError::new(
        ErrorCode::Io,
        format!("Memory database unavailable: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_limits_match_the_documented_budgets() {
        assert_eq!(MAX_MEMORY_RECORDS, 200);
        assert_eq!(MAX_MEMORY_REVISIONS, 50);
        assert_eq!(MAX_MEMORY_BODY_CHARS, 8_000);
    }

    #[test]
    fn the_legacy_namespace_replaces_every_slash() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT, path TEXT)")
            .unwrap();
        assert_eq!(
            namespace(&connection, Path::new("/home/a/b")).unwrap(),
            "-home-a-b"
        );
    }
}
