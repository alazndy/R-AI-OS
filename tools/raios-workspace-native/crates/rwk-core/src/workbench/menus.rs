//! What the context menus and the toolbar offer. Both are lists of catalog commands, so a menu
//! entry, a toolbar button, a shortcut and the palette all go through the same
//! [`available`](super::commands::available) check and carry the same label.
//!
//! A right click selects what it hit before the menu opens (as editors do), so every command
//! here acts on "the selected terminal / the open file" and needs no target argument.

use super::commands::CommandId;

/// Where a context menu was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTarget {
    /// The title (tab) of a terminal.
    TerminalTitle,
    /// The screen of a terminal.
    TerminalBody,
    /// The dock outside any terminal.
    DockBackground,
    /// A tab of the editor.
    EditorTab,
}

/// One line of a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    /// A catalog command.
    Command(CommandId),
    /// A thin rule between groups.
    Separator,
}

use CommandId as C;
use MenuEntry::{Command, Separator};

const TERMINAL_TITLE: &[MenuEntry] = &[
    Command(C::RenameTerminal),
    Separator,
    Command(C::SplitTerminalRight),
    Command(C::SplitTerminalDown),
    Command(C::JoinTerminalPrevious),
    Command(C::UngroupTerminal),
    Separator,
    Command(C::MoveTerminalPrevious),
    Command(C::MoveTerminalNext),
    Command(C::GrowTerminal),
    Command(C::ShrinkTerminal),
    Separator,
    Command(C::Handoff),
    Command(C::NewTask),
    Separator,
    Command(C::StopTerminal),
];

const TERMINAL_BODY: &[MenuEntry] = &[
    Command(C::CopyTerminal),
    Command(C::PasteTerminal),
    Command(C::ClearTerminal),
    Separator,
    Command(C::RenameTerminal),
    Command(C::SplitTerminalRight),
    Command(C::SplitTerminalDown),
    Separator,
    Command(C::Handoff),
    Command(C::NewTask),
    Separator,
    Command(C::StopTerminal),
];

const DOCK_BACKGROUND: &[MenuEntry] = &[
    Command(C::NewTerminal),
    Command(C::Agent),
    Separator,
    Command(C::NewTask),
    Command(C::Handoff),
    Separator,
    Command(C::Inspector),
    Command(C::ToggleTerminal),
];

const EDITOR_TAB: &[MenuEntry] = &[
    Command(C::Save),
    Command(C::Diff),
    Command(C::DiffSplit),
    Separator,
    Command(C::CloseTab),
];

/// The entries of the menu opened on `target`, top to bottom.
pub fn menu_entries(target: MenuTarget) -> &'static [MenuEntry] {
    match target {
        MenuTarget::TerminalTitle => TERMINAL_TITLE,
        MenuTarget::TerminalBody => TERMINAL_BODY,
        MenuTarget::DockBackground => DOCK_BACKGROUND,
        MenuTarget::EditorTab => EDITOR_TAB,
    }
}

/// One element of the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarItem {
    /// A button for a catalog command, with its short caption.
    Button(CommandId, &'static str),
    /// A thin rule between groups.
    Separator,
}

use ToolbarItem::{Button, Separator as Gap};

const TOOLBAR: &[ToolbarItem] = &[
    Button(C::Project, "Proje"),
    Button(C::Files, "Dosya ara"),
    Button(C::Search, "Metin ara"),
    Button(C::Save, "Kaydet"),
    Gap,
    Button(C::NewTerminal, "Terminal +"),
    Button(C::Agent, "Ajan"),
    Button(C::RenameTerminal, "Ad ver"),
    Button(C::SplitTerminalRight, "Böl →"),
    Button(C::SplitTerminalDown, "Böl ↓"),
    Button(C::GrowTerminal, "Büyüt"),
    Button(C::ShrinkTerminal, "Küçült"),
    Button(C::StopTerminal, "Durdur"),
    Gap,
    Button(C::NewTask, "Görev +"),
    Button(C::Handoff, "Devret"),
    Button(C::Memory, "Hafıza"),
    Gap,
    Button(C::Balanced, "Dengeli"),
    Button(C::EditorLayout, "Editör"),
    Button(C::AgentsLayout, "Ajanlar"),
    Button(C::Inspector, "Denetçi"),
    Button(C::ToggleTerminal, "Dock"),
    Gap,
    Button(C::Commands, "Komutlar"),
    Button(C::Settings, "Ayarlar"),
];

/// The toolbar, left to right.
pub fn toolbar() -> &'static [ToolbarItem] {
    TOOLBAR
}

/// Which toolbar buttons show a pressed state, given what the workbench is showing.
pub fn toolbar_pressed(id: CommandId, dock_visible: bool, inspector_visible: bool) -> bool {
    match id {
        CommandId::ToggleTerminal => dock_visible,
        CommandId::Inspector => inspector_visible,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::commands::catalog;

    const TARGETS: [MenuTarget; 4] = [
        MenuTarget::TerminalTitle,
        MenuTarget::TerminalBody,
        MenuTarget::DockBackground,
        MenuTarget::EditorTab,
    ];

    fn in_catalog(id: CommandId) -> bool {
        catalog().iter().any(|command| command.id == id)
    }

    #[test]
    fn every_menu_and_toolbar_command_exists_in_the_catalog() {
        for target in TARGETS {
            for entry in menu_entries(target) {
                if let MenuEntry::Command(id) = entry {
                    assert!(in_catalog(*id), "{target:?}: {id:?}");
                }
            }
        }
        for item in toolbar() {
            if let ToolbarItem::Button(id, _) = item {
                assert!(in_catalog(*id), "{id:?}");
            }
        }
    }

    #[test]
    fn menus_have_no_leading_trailing_or_doubled_rules_and_no_repeats() {
        for target in TARGETS {
            let entries = menu_entries(target);
            assert!(!entries.is_empty());
            assert_ne!(entries.first(), Some(&MenuEntry::Separator), "{target:?}");
            assert_ne!(entries.last(), Some(&MenuEntry::Separator), "{target:?}");
            assert!(
                entries
                    .windows(2)
                    .all(|pair| pair != [MenuEntry::Separator, MenuEntry::Separator]),
                "{target:?}"
            );
            let mut commands: Vec<String> = entries
                .iter()
                .filter_map(|entry| match entry {
                    MenuEntry::Command(id) => Some(format!("{id:?}")),
                    MenuEntry::Separator => None,
                })
                .collect();
            let before = commands.len();
            commands.sort();
            commands.dedup();
            assert_eq!(commands.len(), before, "{target:?} repeats a command");
        }
    }

    #[test]
    fn the_toolbar_has_no_leading_trailing_or_doubled_rules_and_no_repeats() {
        let items = toolbar();
        assert_ne!(items.first(), Some(&ToolbarItem::Separator));
        assert_ne!(items.last(), Some(&ToolbarItem::Separator));
        assert!(items
            .windows(2)
            .all(|pair| pair != [ToolbarItem::Separator, ToolbarItem::Separator]));
        let mut ids: Vec<String> = items
            .iter()
            .filter_map(|item| match item {
                ToolbarItem::Button(id, _) => Some(format!("{id:?}")),
                ToolbarItem::Separator => None,
            })
            .collect();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn captions_are_short_enough_for_a_single_row() {
        for item in toolbar() {
            if let ToolbarItem::Button(_, caption) = item {
                assert!(
                    !caption.is_empty() && caption.chars().count() <= 12,
                    "{caption}"
                );
            }
        }
    }

    #[test]
    fn only_the_panel_toggles_show_a_pressed_state() {
        assert!(toolbar_pressed(CommandId::ToggleTerminal, true, false));
        assert!(!toolbar_pressed(CommandId::ToggleTerminal, false, true));
        assert!(toolbar_pressed(CommandId::Inspector, false, true));
        assert!(!toolbar_pressed(CommandId::Save, true, true));
    }
}
