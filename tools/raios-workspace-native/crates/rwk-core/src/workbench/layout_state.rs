//! What one project had open, as it is stored between runs: its editor tabs, the active tab,
//! the layout preset and its terminal sessions.
//!
//! Pure data plus the parse/sanitize rules; where the file lives and how it is written belongs
//! to the application (`rwk-app` keeps it owner-only and atomic). Everything read back from
//! disk is untrusted: [`parse_workspace`] drops paths that could escape the project, caps the
//! tab and terminal counts and clamps the active index, so a hand-edited file cannot make the
//! workspace open something it should not.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sessions::{SessionKind, SessionName, MAX_SESSIONS};
use crate::workbench::dock::DockLayout;
use crate::workbench::layout::LayoutPreset;

/// Most editor tabs stored for one project. Beyond this the oldest tabs are dropped; the cap
/// only bounds what a hand-written file can ask the workspace to read at startup.
pub const MAX_SAVED_TABS: usize = 32;

/// One editor tab: a project-relative path and the cursor line it was left at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTab {
    /// Project-relative path, `/` separators.
    pub path: String,
    /// 1-based cursor line; older files without one land on the first line.
    #[serde(default = "first_line")]
    pub line: usize,
    /// Pinned tabs sit first in the strip and are kept by "close other tabs".
    #[serde(default)]
    pub pinned: bool,
}

fn first_line() -> usize {
    1
}

/// One terminal session of the saved workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "terminal", rename_all = "snake_case")]
pub enum SavedTerminal {
    /// A plain shell; it is restarted when the project opens.
    Shell {
        /// The name the user gave it, when there is one.
        #[serde(default)]
        name: Option<String>,
    },
    /// An agent session; on restore it is *offered* (through `raios resume` planning) and only
    /// started when the user accepts.
    Agent {
        /// Which agent ran.
        kind: SessionKind,
        /// The agent's own conversation id when it is known.
        #[serde(default)]
        native_session: Option<String>,
        /// The name the user gave the terminal, when there is one.
        #[serde(default)]
        name: Option<String>,
    },
}

impl SavedTerminal {
    /// The stored name, when it passes the same rules as a typed one.
    pub fn name(&self) -> Option<SessionName> {
        let (Self::Shell { name } | Self::Agent { name, .. }) = self;
        name.as_deref().and_then(|raw| SessionName::parse(raw).ok())
    }

    /// The same terminal with its name normalized, or dropped when it is not a valid name: a
    /// hand-edited file cannot put an option-looking or credential-looking text into argv or
    /// the daemon's records.
    fn sanitized(mut self) -> Self {
        let clean = self.name().map(|name| name.to_string());
        let (Self::Shell { name } | Self::Agent { name, .. }) = &mut self;
        *name = clean;
        self
    }
}

/// The task a terminal carries, as stored. The title is only for display; the id is what the
/// daemon knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTaskLink {
    /// Task identifier.
    pub id: String,
    /// Task title at the time of saving.
    pub title: String,
}

impl SavedTaskLink {
    /// The link when it passes the rules a typed task goes through, else `None`: a hand-edited
    /// file cannot put control characters or credential-looking text on screen.
    fn sanitized(self) -> Option<Self> {
        let id_ok = !self.id.is_empty()
            && self.id.chars().count() <= 64
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let title = crate::tasks::validate_title(&self.title).ok()?;
        id_ok.then_some(Self { id: self.id, title })
    }
}

/// Everything one project had open at save time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWorkspace {
    /// Absolute project root this state belongs to.
    pub project: PathBuf,
    /// Open editor tabs, in strip order.
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
    /// Index of the active tab; clamped to the tabs that survive sanitizing.
    #[serde(default)]
    pub active: usize,
    /// The layout preset.
    #[serde(default)]
    pub layout: LayoutPreset,
    /// Terminal sessions.
    #[serde(default)]
    pub terminals: Vec<SavedTerminal>,
    /// The task each terminal carries, by position in `terminals`; shorter than `terminals` (or
    /// absent, in older files) means the rest carry none.
    #[serde(default)]
    pub task_links: Vec<Option<SavedTaskLink>>,
    /// How the dock arranges the terminals; each pane is a position in `terminals`. Absent in
    /// older files, which load with the default arrangement.
    #[serde(default)]
    pub dock: Option<DockLayout<usize>>,
}

/// Reads untrusted state text. `None` when it is not valid JSON, has no absolute project, or
/// nothing usable survives sanitizing.
pub fn parse_workspace(text: &str) -> Option<SavedWorkspace> {
    let workspace: SavedWorkspace = serde_json::from_str(text).ok()?;
    if !workspace.project.is_absolute() {
        return None;
    }
    Some(workspace.sanitized())
}

/// Serializes state for storage. The text round-trips through [`parse_workspace`].
pub fn serialize_workspace(workspace: &SavedWorkspace) -> String {
    serde_json::to_string(workspace).unwrap_or_default()
}

impl SavedWorkspace {
    /// The same workspace with everything a file cannot be trusted with removed: relative-only
    /// tab paths without `..` or NUL, no duplicates, capped counts, active index clamped.
    pub fn sanitized(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        self.tabs
            .retain(|tab| valid_tab_path(&tab.path) && seen.insert(tab.path.clone()));
        self.tabs.truncate(MAX_SAVED_TABS);
        // Pinned tabs lead the strip: a pin after an unpinned tab is not honoured.
        let mut leading = true;
        for tab in &mut self.tabs {
            leading &= tab.pinned;
            tab.pinned = leading;
        }
        self.terminals.truncate(MAX_SESSIONS);
        self.task_links.truncate(self.terminals.len());
        self.task_links = self
            .task_links
            .into_iter()
            .map(|link| link.and_then(SavedTaskLink::sanitized))
            .collect();
        self.terminals = self
            .terminals
            .into_iter()
            .map(SavedTerminal::sanitized)
            .collect();
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        // Panes that point past the terminals are dropped and repeats are cleaned; a layout
        // that cannot be repaired (nested too deeply) or has nothing left falls back to the
        // dock's default arrangement.
        let terminals = self.terminals.len();
        self.dock = self
            .dock
            .take()
            .map(|dock| dock.filter_map(&|index: &usize| (*index < terminals).then_some(*index)))
            .filter(|dock| !dock.is_empty());
        self
    }
}

/// True when `path` is a project-relative document path: not empty, not absolute, with no NUL
/// and no `..` segment (the reader would refuse such paths anyway; this keeps them out of state).
fn valid_tab_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\0')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

/// File name the state of `project` is stored under: the lowercase hex SHA-256 of the project
/// path plus `.json`. Hashing keeps user paths out of file names and gives one file per project.
pub fn state_file_name(project: &Path) -> String {
    let digest = Sha256::digest(project.to_string_lossy().as_bytes());
    format!("{digest:x}.json")
}

/// Where the workspace loads its saved state from when a project opens. Implemented by the
/// application over the real files; tests use an in-memory stand-in.
pub trait WorkspaceStateSource: Send + Sync {
    /// The state stored for `project`, when there is a valid one.
    fn load(&self, project: &Path) -> Option<SavedWorkspace>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> SavedWorkspace {
        SavedWorkspace {
            project: PathBuf::from("/work/app"),
            tabs: vec![
                SavedTab {
                    path: "src/main.rs".into(),
                    line: 12,
                    pinned: false,
                },
                SavedTab {
                    path: "README.md".into(),
                    line: 1,
                    pinned: false,
                },
            ],
            active: 1,
            layout: LayoutPreset::Agents,
            dock: None,
            terminals: vec![
                SavedTerminal::Shell { name: None },
                SavedTerminal::Agent {
                    kind: SessionKind::Codex,
                    native_session: Some("sess-1".into()),
                    name: None,
                },
            ],
            task_links: Vec::new(),
        }
    }

    #[test]
    fn a_workspace_round_trips_through_its_text() {
        let state = workspace();
        let parsed = parse_workspace(&serialize_workspace(&state)).expect("parse");
        assert_eq!(parsed, state);
    }

    #[test]
    fn the_wire_shape_is_stable() {
        let text = serialize_workspace(&workspace());
        assert!(text.contains("\"terminal\":\"shell\""), "{text}");
        assert!(
            text.contains("\"terminal\":\"agent\",\"kind\":\"codex\""),
            "{text}"
        );
        assert!(text.contains("\"layout\":\"agents\""), "{text}");
    }

    #[test]
    fn a_minimal_file_gets_every_default() {
        let parsed = parse_workspace(r#"{"project":"/work/app"}"#).expect("parse");
        assert!(parsed.tabs.is_empty());
        assert_eq!(parsed.active, 0);
        assert_eq!(parsed.layout, LayoutPreset::Balanced);
        assert!(parsed.terminals.is_empty());

        // A tab without a line lands on the first line.
        let parsed =
            parse_workspace(r#"{"project":"/work/app","tabs":[{"path":"a.rs"}]}"#).expect("parse");
        assert_eq!(parsed.tabs[0].line, 1);
    }

    #[test]
    fn only_the_leading_run_of_pinned_tabs_is_kept() {
        let parsed = parse_workspace(
            r#"{"project":"/work/app","tabs":[
                {"path":"a.rs","pinned":true},
                {"path":"b.rs"},
                {"path":"c.rs","pinned":true}
            ]}"#,
        )
        .expect("parse");
        let pins: Vec<bool> = parsed.tabs.iter().map(|tab| tab.pinned).collect();
        assert_eq!(pins, [true, false, false]);
    }

    #[test]
    fn junk_or_relative_projects_are_refused() {
        assert!(parse_workspace("not json").is_none());
        assert!(parse_workspace("[]").is_none());
        assert!(parse_workspace(r#"{"tabs":[]}"#).is_none());
        assert!(parse_workspace(r#"{"project":"relative/dir"}"#).is_none());
    }

    #[test]
    fn tab_paths_that_could_escape_the_project_are_dropped() {
        let parsed = parse_workspace(
            r#"{"project":"/work/app","tabs":[
                {"path":"src/lib.rs"},
                {"path":"/etc/passwd"},
                {"path":"../outside"},
                {"path":"a/../../b"},
                {"path":""},
                {"path":"src/lib.rs"}
            ],"active":5}"#,
        )
        .expect("parse");

        assert_eq!(
            parsed.tabs,
            vec![SavedTab {
                path: "src/lib.rs".into(),
                line: 1,
                pinned: false,
            }]
        );
        assert_eq!(parsed.active, 0, "clamped to the surviving tabs");
    }

    #[test]
    fn counts_are_capped_to_what_the_workspace_can_open() {
        let tabs: Vec<SavedTab> = (0..100)
            .map(|index| SavedTab {
                path: format!("file-{index}.rs"),
                line: 1,
                pinned: false,
            })
            .collect();
        let more_terminals: Vec<SavedTerminal> = (0..MAX_SESSIONS + 4)
            .map(|_| SavedTerminal::Shell { name: None })
            .collect();
        let parsed = SavedWorkspace {
            project: PathBuf::from("/work/app"),
            tabs,
            active: 99,
            layout: LayoutPreset::Balanced,
            dock: None,
            terminals: more_terminals,
            task_links: Vec::new(),
        }
        .sanitized();

        assert_eq!(parsed.tabs.len(), MAX_SAVED_TABS);
        assert_eq!(parsed.terminals.len(), MAX_SESSIONS);
        assert!(parsed.active < parsed.tabs.len());
    }

    #[test]
    fn the_state_file_name_is_stable_hex_and_one_per_project() {
        let name = state_file_name(Path::new("/work/app"));
        assert_eq!(name, state_file_name(Path::new("/work/app")));
        assert_ne!(name, state_file_name(Path::new("/work/other")));
        let stem = name.strip_suffix(".json").expect("json suffix");
        assert_eq!(stem.len(), 64, "sha-256 hex: {name}");
        assert!(stem.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
    }

    #[test]
    fn the_dock_arrangement_is_kept_cleaned_and_absent_in_old_files() {
        use crate::workbench::dock::Edge;

        let old = parse_workspace(r#"{"project":"/work/app","terminals":[{"terminal":"shell"}]}"#)
            .expect("parse");
        assert_eq!(old.dock, None, "an older file has no arrangement");

        let mut dock = DockLayout::stacked([0_usize, 1, 2]);
        dock.split(&2, &0, Edge::Right).unwrap();
        let mut workspace = workspace();
        workspace.terminals = vec![SavedTerminal::Shell { name: None }; 3];
        workspace.dock = Some(dock.clone());
        let again = parse_workspace(&serialize_workspace(&workspace)).expect("round trip");
        assert_eq!(again.dock, Some(dock));

        // Positions past the terminals are dropped and the rest closes up.
        workspace.terminals = vec![SavedTerminal::Shell { name: None }; 2];
        let cleaned = workspace.clone().sanitized();
        assert_eq!(
            cleaned.dock.as_ref().map(DockLayout::sessions),
            Some(vec![0, 1])
        );

        // Nothing usable left: the dock falls back to its default arrangement.
        workspace.terminals.clear();
        assert_eq!(workspace.sanitized().dock, None);
    }

    #[test]
    fn task_links_are_cleaned_capped_and_optional_in_old_files() {
        // A file from before task links still loads.
        let old = parse_workspace(r#"{"project":"/work/app","terminals":[{"terminal":"shell"}]}"#)
            .expect("parses");
        assert!(old.task_links.is_empty());

        let mut state = workspace();
        let link = |id: &str, title: &str| {
            Some(SavedTaskLink {
                id: id.into(),
                title: title.into(),
            })
        };
        state.task_links = vec![
            link("t-1", "  Derle  "),
            link("bad id with spaces", "x"),
            link("t-3", "tab\there"),
            link(
                "t-4",
                "token sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
            ),
        ];
        // More links than terminals are cut.
        state
            .task_links
            .extend((0..20).map(|n| link(&format!("t-{n}"), "extra")));
        let clean = state.sanitized();
        assert!(clean.task_links.len() <= clean.terminals.len());
        assert_eq!(
            clean.task_links[0].as_ref().map(|l| l.title.as_str()),
            Some("Derle")
        );
        assert!(clean.task_links[1..].iter().all(Option::is_none) || clean.task_links.len() < 2);
    }
}
