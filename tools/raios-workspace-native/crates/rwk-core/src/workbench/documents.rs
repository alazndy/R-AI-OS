//! The open-document session: dirty tracking, conflict handling and the guard that stops edits
//! from being lost when the user switches file, project or window. Pure state; the editor owns
//! the text and tells the session what happened.

use crate::documents::{Document, Revision};
use crate::error::{AppError, ErrorCode};
use crate::project::ProjectId;

/// Whether the user may leave the open document right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchDecision {
    /// Nothing would be lost.
    Allowed,
    /// There are unsaved edits: ask the user to save, discard or cancel.
    NeedsDecision,
}

/// Why a save cannot start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveBlocked {
    /// No file is open.
    NoDocument,
    /// There is nothing to save.
    Unchanged,
    /// The file changed on disk; reload or reconcile first.
    Conflict,
}

/// Identifies one save attempt, so edits made while it was running are not marked as saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveTicket(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Open {
    project_id: ProjectId,
    path: String,
    base: Revision,
    dirty: bool,
    conflict: bool,
    /// Counts edits; a save only clears `dirty` when no edit happened since it began.
    generation: u64,
    last_error: Option<ErrorCode>,
}

/// State of the file open in the editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentSession {
    open: Option<Open>,
}

impl DocumentSession {
    /// A session with no file open.
    pub fn new() -> Self {
        Self::default()
    }

    /// Path of the open file, relative to its project.
    pub fn path(&self) -> Option<&str> {
        self.open.as_ref().map(|open| open.path.as_str())
    }

    /// True when the open file has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.dirty)
    }

    /// True when the last save found the file changed on disk.
    pub fn has_conflict(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.conflict)
    }

    /// The code of the last failed save that was not a conflict.
    pub fn last_error(&self) -> Option<ErrorCode> {
        self.open.as_ref().and_then(|open| open.last_error)
    }

    /// Whether the user may open `path` of `project` now. Reopening the same file is always
    /// allowed (it is a no-op for the caller); anything else needs a decision while dirty.
    pub fn request_open(&self, project: &ProjectId, path: &str) -> SwitchDecision {
        match &self.open {
            Some(open) if open.dirty && (open.project_id != *project || open.path != path) => {
                SwitchDecision::NeedsDecision
            }
            _ => SwitchDecision::Allowed,
        }
    }

    /// Whether the window, project or app may close now.
    pub fn request_leave(&self) -> SwitchDecision {
        if self.is_dirty() {
            SwitchDecision::NeedsDecision
        } else {
            SwitchDecision::Allowed
        }
    }

    /// A document was read from disk and loaded into the editor (also used to reload).
    pub fn opened(&mut self, document: &Document) {
        self.open = Some(Open {
            project_id: document.project_id.clone(),
            path: document.path.clone(),
            base: document.revision.clone(),
            dirty: false,
            conflict: false,
            generation: 0,
            last_error: None,
        });
    }

    /// The user changed the text.
    pub fn edited(&mut self) {
        if let Some(open) = &mut self.open {
            open.dirty = true;
            open.generation += 1;
        }
    }

    /// Starts a save: the document to write (the editor's current `text` on top of the revision
    /// the file was read at) and the ticket to hand back on success.
    pub fn begin_save(&self, text: &str) -> Result<(Document, SaveTicket), SaveBlocked> {
        let open = self.open.as_ref().ok_or(SaveBlocked::NoDocument)?;
        if open.conflict {
            return Err(SaveBlocked::Conflict);
        }
        if !open.dirty {
            return Err(SaveBlocked::Unchanged);
        }
        Ok((
            Document {
                project_id: open.project_id.clone(),
                path: open.path.clone(),
                content: text.to_owned(),
                revision: open.base.clone(),
            },
            SaveTicket(open.generation),
        ))
    }

    /// The write succeeded; `saved` carries the new revision. Edits made while the save ran keep
    /// the file dirty.
    pub fn save_succeeded(&mut self, saved: &Document, ticket: SaveTicket) {
        if let Some(open) = &mut self.open {
            if open.project_id == saved.project_id && open.path == saved.path {
                open.base = saved.revision.clone();
                open.conflict = false;
                open.last_error = None;
                if open.generation == ticket.0 {
                    open.dirty = false;
                }
            }
        }
    }

    /// The write failed. A conflict keeps the edits and blocks further saves until the file is
    /// reloaded; any other error keeps the file dirty and records its code.
    pub fn save_failed(&mut self, error: &AppError) {
        if let Some(open) = &mut self.open {
            if error.code == ErrorCode::Conflict {
                open.conflict = true;
            } else {
                open.last_error = Some(error.code);
            }
        }
    }

    /// The user chose to throw the edits away and close the file.
    pub fn close(&mut self) {
        self.open = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(path: &str, revision: &str) -> Document {
        Document {
            project_id: ProjectId::new("p1"),
            path: path.into(),
            content: "text".into(),
            revision: Revision::new(revision),
        }
    }

    fn open_dirty() -> DocumentSession {
        let mut session = DocumentSession::new();
        session.opened(&document("src/lib.rs", "r1"));
        session.edited();
        session
    }

    #[test]
    fn a_clean_file_can_be_left_and_a_dirty_one_needs_a_decision() {
        let mut session = DocumentSession::new();
        let project = ProjectId::new("p1");
        assert_eq!(
            session.request_open(&project, "a.rs"),
            SwitchDecision::Allowed
        );
        session.opened(&document("a.rs", "r1"));
        assert_eq!(session.request_leave(), SwitchDecision::Allowed);
        session.edited();
        assert_eq!(
            session.request_open(&project, "b.rs"),
            SwitchDecision::NeedsDecision
        );
        assert_eq!(
            session.request_open(&ProjectId::new("p2"), "a.rs"),
            SwitchDecision::NeedsDecision
        );
        assert_eq!(
            session.request_open(&project, "a.rs"),
            SwitchDecision::Allowed
        );
        assert_eq!(session.request_leave(), SwitchDecision::NeedsDecision);
    }

    #[test]
    fn saving_needs_an_open_dirty_file() {
        let mut session = DocumentSession::new();
        assert_eq!(
            session.begin_save("x").unwrap_err(),
            SaveBlocked::NoDocument
        );
        session.opened(&document("a.rs", "r1"));
        assert_eq!(session.begin_save("x").unwrap_err(), SaveBlocked::Unchanged);
        session.edited();
        let (document, _) = session.begin_save("new text").unwrap();
        assert_eq!(document.content, "new text");
        assert_eq!(document.revision, Revision::new("r1"));
        assert_eq!(document.path, "a.rs");
    }

    #[test]
    fn a_successful_save_moves_the_base_revision_and_clears_dirty() {
        let mut session = open_dirty();
        let (_, ticket) = session.begin_save("v2").unwrap();
        session.save_succeeded(&document("src/lib.rs", "r2"), ticket);
        assert!(!session.is_dirty());
        session.edited();
        let (next, _) = session.begin_save("v3").unwrap();
        assert_eq!(next.revision, Revision::new("r2"));
    }

    #[test]
    fn edits_made_while_a_save_runs_keep_the_file_dirty() {
        let mut session = open_dirty();
        let (_, ticket) = session.begin_save("v2").unwrap();
        session.edited();
        session.save_succeeded(&document("src/lib.rs", "r2"), ticket);
        assert!(session.is_dirty());
        let (next, _) = session.begin_save("v3").unwrap();
        assert_eq!(next.revision, Revision::new("r2"));
    }

    #[test]
    fn a_conflict_keeps_the_edits_and_blocks_saving_until_reload() {
        let mut session = open_dirty();
        session.save_failed(&AppError::new(ErrorCode::Conflict, "changed on disk"));
        assert!(session.is_dirty());
        assert!(session.has_conflict());
        assert_eq!(session.begin_save("x").unwrap_err(), SaveBlocked::Conflict);
        assert_eq!(session.request_leave(), SwitchDecision::NeedsDecision);
        session.opened(&document("src/lib.rs", "r9"));
        assert!(!session.has_conflict());
        assert!(!session.is_dirty());
    }

    #[test]
    fn another_failure_keeps_the_file_dirty_and_records_the_code() {
        let mut session = open_dirty();
        session.save_failed(&AppError::new(ErrorCode::InvalidInput, "too large"));
        assert!(session.is_dirty());
        assert!(!session.has_conflict());
        assert_eq!(session.last_error(), Some(ErrorCode::InvalidInput));
        assert!(session.begin_save("x").is_ok());
    }

    #[test]
    fn a_stale_save_result_for_another_file_is_ignored() {
        let mut session = open_dirty();
        let (_, ticket) = session.begin_save("v2").unwrap();
        session.opened(&document("other.rs", "r5"));
        session.edited();
        session.save_succeeded(&document("src/lib.rs", "r2"), ticket);
        assert!(session.is_dirty());
        let (next, _) = session.begin_save("v").unwrap();
        assert_eq!(next.revision, Revision::new("r5"));
    }
}
