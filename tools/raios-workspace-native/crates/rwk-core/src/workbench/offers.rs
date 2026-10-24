//! Previous agent sessions of a project: the `raios resume --list --json` catalog is parsed and
//! matched against the agent terminals a saved workspace remembers, so the dock can offer to
//! continue a conversation instead of starting a new one.
//!
//! Planning is read-only and best-effort. The catalog is the CLI's own output, and every offer
//! is re-checked when it is taken: `raios resume <key>` plans again from a fresh scan and
//! refuses a conversation that disappeared or became unavailable since the list was read.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sessions::{ResumeKey, SessionKind, SessionName};
use crate::workbench::layout_state::SavedTerminal;

/// One row of the recovery catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSessionRow {
    /// Full `agent:native-id` key.
    pub key: String,
    /// Agent name as the catalog spells it (`claude`, `codex`, `opencode`).
    pub agent: String,
    /// The agent's own conversation id, when the row has one.
    #[serde(default)]
    pub native_session_id: Option<String>,
    /// Absolute path the conversation ran in.
    #[serde(default)]
    pub project_path: String,
    /// Display name of the conversation.
    #[serde(default)]
    pub name: String,
    /// Whether the conversation can be continued safely right now.
    #[serde(default)]
    pub resumable: bool,
}

/// The catalog as `raios resume --list --json` prints it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCatalog {
    /// Saved conversations, newest first.
    #[serde(default)]
    pub sessions: Vec<SavedSessionRow>,
    /// Non-fatal scan warnings from the CLI.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Parses the catalog JSON; `None` when the text is not one (the CLI missing, an error message
/// on standard output, a version that prints something else).
pub fn parse_catalog(text: &str) -> Option<SessionCatalog> {
    serde_json::from_str(text).ok()
}

/// A catalog conversation matched to a saved agent pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedSession {
    /// Validated key the continuation pane runs with.
    pub key: ResumeKey,
    /// Display name of the conversation.
    pub name: String,
}

/// One previous agent session offered for continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOffer {
    /// The agent the saved pane ran.
    pub kind: SessionKind,
    /// The conversation id from the saved state, when there was one.
    pub native_session: Option<String>,
    /// The name the pane had, when it had one.
    pub name: Option<SessionName>,
    /// The resumable catalog row this offer belongs to, when one was matched.
    pub matched: Option<MatchedSession>,
}

/// Matches the agent terminals of a saved workspace against the catalog of `project`.
///
/// Rows are only ever taken for their exact project. A pane whose conversation id is known takes
/// exactly that conversation back; a pane without one takes the newest resumable conversation of
/// the same agent that no other offer claimed (the catalog is newest first). An id that left the
/// catalog is not replaced with a different conversation: the user saved *that* session.
pub fn plan_offers(
    saved: &[SavedTerminal],
    catalog: &SessionCatalog,
    project: &Path,
) -> Vec<SessionOffer> {
    let project = project.to_string_lossy();
    let rows: Vec<&SavedSessionRow> = catalog
        .sessions
        .iter()
        .filter(|row| row.project_path == project && row.resumable)
        .collect();
    let mut claimed: HashSet<&str> = HashSet::new();
    let mut offers = Vec::new();
    for terminal in saved {
        let SavedTerminal::Agent {
            kind,
            native_session,
            ..
        } = terminal
        else {
            continue;
        };
        let mut matched = None;
        if let Some(agent) = kind.native_agent() {
            let row = match native_session {
                Some(id) => rows.iter().copied().find(|row| {
                    row.agent == agent
                        && row.native_session_id.as_deref() == Some(id.as_str())
                        && !claimed.contains(row.key.as_str())
                }),
                None => rows
                    .iter()
                    .copied()
                    .find(|row| row.agent == agent && !claimed.contains(row.key.as_str())),
            };
            if let Some(row) = row {
                if let Some(key) = ResumeKey::parse(&row.key) {
                    claimed.insert(row.key.as_str());
                    matched = Some(MatchedSession {
                        key,
                        name: row.name.clone(),
                    });
                }
            }
        }
        offers.push(SessionOffer {
            kind: *kind,
            native_session: native_session.clone(),
            name: terminal.name(),
            matched,
        });
    }
    offers
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_A: &str = "claude:3294c005-aee0-4e95-860f-5800e045b624";
    const CLAUDE_B: &str = "claude:4e950075-aee0-4e95-860f-5800e045b624";
    const CODEX_A: &str = "codex:9ac8a20a-7f0d-4c11-9f31-6b6e2f2ab111";
    const PROJECT: &str = "/work/app";

    fn row(key: &str, project: &str, name: &str, resumable: bool) -> SavedSessionRow {
        let (agent, id) = key.split_once(':').expect("test key");
        SavedSessionRow {
            key: key.into(),
            agent: agent.into(),
            native_session_id: Some(id.into()),
            project_path: project.into(),
            name: name.into(),
            resumable,
        }
    }

    fn catalog(sessions: Vec<SavedSessionRow>) -> SessionCatalog {
        SessionCatalog {
            sessions,
            warnings: Vec::new(),
        }
    }

    fn agent(kind: SessionKind, native: Option<&str>) -> SavedTerminal {
        SavedTerminal::Agent {
            kind,
            native_session: native.map(str::to_owned),
            name: None,
        }
    }

    #[test]
    fn a_saved_id_takes_exactly_that_conversation_back() {
        let saved = [agent(
            SessionKind::Claude,
            Some("4e950075-aee0-4e95-860f-5800e045b624"),
        )];
        let rows = catalog(vec![
            row(CLAUDE_A, PROJECT, "newer", true),
            row(CLAUDE_B, PROJECT, "saved one", true),
        ]);

        let offers = plan_offers(&saved, &rows, Path::new(PROJECT));

        let matched = offers[0].matched.as_ref().expect("matched");
        assert_eq!(matched.key.to_string(), CLAUDE_B, "not the newest");
        assert_eq!(matched.name, "saved one");
        assert_eq!(
            offers[0].native_session.as_deref(),
            Some("4e950075-aee0-4e95-860f-5800e045b624")
        );
    }

    #[test]
    fn ids_missing_from_the_catalog_are_not_replaced() {
        let saved = [agent(
            SessionKind::Claude,
            Some("00000000-0000-0000-0000-000000000000"),
        )];
        let rows = catalog(vec![row(CLAUDE_A, PROJECT, "another one", true)]);

        let offers = plan_offers(&saved, &rows, Path::new(PROJECT));

        assert!(offers[0].matched.is_none());
        assert_eq!(
            offers[0].native_session.as_deref(),
            Some("00000000-0000-0000-0000-000000000000"),
            "the saved id is kept for the next try"
        );
    }

    #[test]
    fn missing_ids_take_the_newest_unclaimed_conversation_of_the_agent() {
        let saved = [
            agent(SessionKind::Claude, None),
            agent(SessionKind::Claude, None),
            agent(SessionKind::Codex, None),
        ];
        let rows = catalog(vec![
            row(CLAUDE_A, PROJECT, "newest claude", true),
            row(CODEX_A, PROJECT, "the codex", true),
            row(CLAUDE_B, PROJECT, "older claude", true),
        ]);

        let offers = plan_offers(&saved, &rows, Path::new(PROJECT));

        let keys: Vec<Option<String>> = offers
            .iter()
            .map(|offer| offer.matched.as_ref().map(|m| m.key.to_string()))
            .collect();
        assert_eq!(
            keys,
            [
                Some(CLAUDE_A.to_owned()),
                Some(CLAUDE_B.to_owned()),
                Some(CODEX_A.to_owned()),
            ],
            "each row at most once, newest first"
        );
    }

    #[test]
    fn only_resumable_rows_of_the_exact_project_are_offered() {
        let saved = [
            agent(SessionKind::Claude, None),
            agent(SessionKind::Codex, None),
        ];
        let rows = catalog(vec![
            row(CLAUDE_A, "/elsewhere/other", "wrong project", true),
            row(CLAUDE_A, PROJECT, "not available", false),
            row(CODEX_A, PROJECT, "available", true),
        ]);

        let offers = plan_offers(&saved, &rows, Path::new(PROJECT));

        assert!(offers[0].matched.is_none(), "unresumable or alien rows");
        assert_eq!(
            offers[1].matched.as_ref().map(|m| m.key.to_string()),
            Some(CODEX_A.to_owned())
        );
    }

    #[test]
    fn agents_without_native_continuation_are_never_matched() {
        let saved = [
            agent(SessionKind::Agy, None),
            SavedTerminal::Shell { name: None },
        ];
        let rows = catalog(vec![row(CLAUDE_A, PROJECT, "available", true)]);

        let offers = plan_offers(&saved, &rows, Path::new(PROJECT));

        assert_eq!(offers.len(), 1, "a shell is not an offer");
        assert!(offers[0].matched.is_none());
    }

    #[test]
    fn a_row_whose_key_does_not_validate_is_left_alone() {
        let mut weird = row(CLAUDE_A, PROJECT, "weird", true);
        weird.key = "claude:-rf".into();
        let saved = [agent(SessionKind::Claude, None)];

        let offers = plan_offers(&saved, &catalog(vec![weird]), Path::new(PROJECT));

        assert!(offers[0].matched.is_none());
    }

    #[test]
    fn the_cli_output_parses_with_its_extra_fields() {
        let text = r#"{
          "sessions": [{
            "key": "claude:3294c005-aee0-4e95-860f-5800e045b624",
            "agent": "claude",
            "native_session_id": "3294c005-aee0-4e95-860f-5800e045b624",
            "project_path": "/work/app",
            "name": "claude 3294c005",
            "work": "Wrapper-routed interactive session for claude_kaira",
            "updated_at": "2026-10-15T19:18:04.532908709+00:00",
            "wrapper_run_id": "a1342ed3-60cb-4b0d-9ac8-a20ab71fdb53",
            "availability": "saved",
            "resumable": true
          }],
          "warnings": []
        }"#;

        let catalog = parse_catalog(text).expect("catalog");

        assert_eq!(catalog.sessions.len(), 1);
        assert!(catalog.sessions[0].resumable);
        assert_eq!(catalog.sessions[0].name, "claude 3294c005");
        assert!(parse_catalog("Session recovery error: no database").is_none());
        assert!(parse_catalog("").is_none());
    }
}
