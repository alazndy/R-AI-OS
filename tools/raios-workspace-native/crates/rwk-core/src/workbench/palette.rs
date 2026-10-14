//! The command palette's content: which commands match a query and why some are unavailable.

use super::commands::{available, catalog, CommandId, CommandState, WorkbenchState};

/// One row of the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteEntry {
    /// The command.
    pub id: CommandId,
    /// Its label.
    pub label: &'static str,
    /// Shortcut text, when it has one.
    pub shortcut: Option<&'static str>,
    /// Whether it can run now and the reason when it cannot.
    pub state: CommandState,
}

/// Lower-cases and folds the Turkish letters to ASCII so `editor` finds `Editör` and `komut`
/// finds `Hızlı komutlar`.
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ı' | 'i' | '\u{307}' => 'i',
            'ş' => 's',
            'ç' => 'c',
            'ğ' => 'g',
            'ö' => 'o',
            'ü' => 'u',
            other => other,
        })
        .filter(|c| *c != '\u{307}')
        .collect()
}

/// The commands whose label contains every word of `query` (an empty query matches all).
/// Commands that can run come first, then the ones that cannot (with their reason); within each
/// group labels that start with the first word come before the rest, and ties keep catalog order.
pub fn palette_entries(query: &str, state: &WorkbenchState) -> Vec<PaletteEntry> {
    let folded = fold(query);
    let words: Vec<&str> = folded.split_whitespace().collect();
    let mut entries: Vec<(bool, bool, usize, PaletteEntry)> = catalog()
        .iter()
        .enumerate()
        .filter_map(|(index, command)| {
            let label = fold(command.label);
            if !words.iter().all(|word| label.contains(word)) {
                return None;
            }
            let starts = words.first().is_some_and(|word| label.starts_with(word));
            let command_state = available(command.id, state);
            Some((
                !command_state.enabled,
                !starts,
                index,
                PaletteEntry {
                    id: command.id,
                    label: command.label,
                    shortcut: command.shortcut,
                    state: command_state,
                },
            ))
        })
        .collect();
    entries.sort_by_key(|(disabled, not_starting, index, _)| (*disabled, *not_starting, *index));
    entries.into_iter().map(|(_, _, _, entry)| entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_project() -> WorkbenchState {
        WorkbenchState {
            project_open: true,
            ..WorkbenchState::default()
        }
    }

    #[test]
    fn an_empty_query_lists_every_command() {
        assert_eq!(
            palette_entries("", &WorkbenchState::default()).len(),
            catalog().len()
        );
    }

    #[test]
    fn the_query_matches_every_word_ignoring_case_and_turkish_letters() {
        let state = open_project();
        let ids: Vec<CommandId> = palette_entries("EDITOR yer", &state)
            .iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(ids, vec![CommandId::EditorLayout]);
        let komut: Vec<CommandId> = palette_entries("komut", &state)
            .iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(komut, vec![CommandId::Commands]);
        assert!(palette_entries("zzz-no-such", &state).is_empty());
    }

    #[test]
    fn unavailable_commands_come_last_and_say_why() {
        let entries = palette_entries("dosya", &WorkbenchState::default());
        let save = entries
            .iter()
            .find(|entry| entry.id == CommandId::Save)
            .expect("save matches");
        assert!(!save.state.enabled);
        assert_eq!(save.state.reason, Some("Açık dosya yok"));
        let first_disabled = entries.iter().position(|entry| !entry.state.enabled);
        let last_enabled = entries.iter().rposition(|entry| entry.state.enabled);
        if let (Some(disabled), Some(enabled)) = (first_disabled, last_enabled) {
            assert!(enabled < disabled, "enabled commands must come first");
        }
    }

    #[test]
    fn a_label_starting_with_the_word_comes_before_one_that_only_contains_it() {
        let entries = palette_entries("terminal", &open_project());
        let first_label = entries.first().expect("matches").label;
        assert!(fold(first_label).starts_with("terminal"), "{first_label}");
    }
}
