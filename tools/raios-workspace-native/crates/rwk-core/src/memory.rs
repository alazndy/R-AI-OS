//! Read-only projection of a project's RAIOS memory records (`mem_items`, `mem_nodes`,
//! `mem_lineage`), as shown by the memory view. Never writes, migrates or touches usage and
//! confidence counters.

use std::path::PathBuf;

use crate::error::Result;
use crate::project::{ProjectId, ProjectRoot};

/// Most records one snapshot loads.
pub const MAX_MEMORY_RECORDS: usize = 200;
/// Most archived revisions one history loads.
pub const MAX_MEMORY_REVISIONS: usize = 50;
/// Most characters of one record body or revision that are loaded.
pub const MAX_MEMORY_BODY_CHARS: usize = 8_000;

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

/// [`MemorySource`] backed by a read-only SQLite connection to `workspace.db`.
pub struct SqliteMemorySource {
    // Read by the P1b implementation.
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl SqliteMemorySource {
    /// Creates a source for the database at `db_path`. Nothing is opened until a query runs.
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }
}

impl MemorySource for SqliteMemorySource {
    fn snapshot(&self, _project: &ProjectRoot) -> Result<MemorySnapshot> {
        unimplemented!("P1b")
    }

    fn history(&self, _project: &ProjectRoot, _slug: &str) -> Result<MemoryHistory> {
        unimplemented!("P1b")
    }
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
}
