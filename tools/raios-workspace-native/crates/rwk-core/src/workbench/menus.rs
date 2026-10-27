//! What the context menus and the toolbar offer. Both are lists of catalog commands, so a menu
//! entry, a toolbar button, a shortcut and the palette all go through the same
//! [`available`](super::commands::available) check and carry the same label.
//!
//! A right click selects what it hit before the menu opens (as editors do), so every command
//! here acts on "the selected terminal / the open file" and needs no target argument.

use serde::{Deserialize, Serialize};

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
    Command(C::Reconnect),
    Command(C::StopTerminal),
    Command(C::CloseTerminal),
    Command(C::KillTerminal),
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
    Command(C::Reconnect),
    Command(C::StopTerminal),
    Command(C::CloseTerminal),
    Command(C::KillTerminal),
];

const DOCK_BACKGROUND: &[MenuEntry] = &[
    Command(C::NewTerminal),
    Command(C::Ssh),
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

/// The category a run of toolbar buttons belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolbarGroup {
    /// Files, search, saving, git and problems.
    Files,
    /// Terminals and the tasks that run in them.
    Terminal,
    /// Agent tasks, handoffs and project memory.
    Work,
    /// Layouts and which panes show.
    View,
    /// The palette and settings.
    Tools,
}

impl ToolbarGroup {
    /// The categories in toolbar order; `ToolbarTab(n)` shows `ALL[n - 1]`.
    pub const ALL: [Self; 5] = [
        Self::Files,
        Self::Terminal,
        Self::Work,
        Self::View,
        Self::Tools,
    ];

    /// The label printed at the start of the group.
    pub fn label(self) -> &'static str {
        match self {
            Self::Files => "DOSYA",
            Self::Terminal => "TERMİNAL",
            Self::Work => "İŞ",
            Self::View => "GÖRÜNÜM",
            Self::Tools => "ARAÇ",
        }
    }
}

/// One element of the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarItem {
    /// A button for a catalog command, with its short caption.
    Button(CommandId, &'static str),
    /// Starts a group: the buttons after it, up to the next group, belong to it.
    Group(ToolbarGroup),
}

use ToolbarGroup as G;
use ToolbarItem::{Button, Group};

const TOOLBAR: &[ToolbarItem] = &[
    Group(G::Files),
    Button(C::Project, "Proje"),
    Button(C::Files, "Dosya ara"),
    Button(C::Search, "Metin ara"),
    Button(C::Save, "Kaydet"),
    Button(C::SourceControl, "Git"),
    Button(C::Problems, "Sorunlar"),
    Group(G::Terminal),
    Button(C::NewTerminal, "Terminal +"),
    Button(C::Ssh, "SSH"),
    Button(C::Agent, "Ajan"),
    Button(C::RunTask, "Çalıştır"),
    Button(C::RenameTerminal, "Ad ver"),
    Button(C::SplitTerminalRight, "Böl →"),
    Button(C::SplitTerminalDown, "Böl ↓"),
    Button(C::GrowTerminal, "Büyüt"),
    Button(C::ShrinkTerminal, "Küçült"),
    Button(C::StopTerminal, "Durdur"),
    Button(C::CloseTerminal, "Kapat"),
    Group(G::Work),
    Button(C::NewTask, "Görev +"),
    Button(C::Handoff, "Devret"),
    Button(C::Memory, "Hafıza"),
    Group(G::View),
    Button(C::Balanced, "Dengeli"),
    Button(C::EditorLayout, "Editör"),
    Button(C::AgentsLayout, "Ajanlar"),
    Button(C::SplitEditor, "Editörü böl"),
    Button(C::Inspector, "Denetçi"),
    Button(C::ToggleTerminal, "Dock"),
    Group(G::Tools),
    Button(C::Commands, "Komutlar"),
    Button(C::Settings, "Ayarlar"),
];

/// How the toolbar arranges its categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolbarMode {
    /// One tab per category; only the open category's buttons show.
    Tabs,
    /// Every shown category in one row, told apart by a coloured rule.
    Flat,
}

/// A ready-made toolbar layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarPreset {
    /// Category tabs, button names and shortcut hints.
    Tabbed,
    /// Category tabs with icons only.
    Compact,
    /// All categories in one row, icons only.
    Flat,
    /// Only the essential buttons, icons only.
    Minimal,
}

impl ToolbarPreset {
    /// The presets in the order the settings list them.
    pub const ALL: [Self; 4] = [Self::Tabbed, Self::Compact, Self::Flat, Self::Minimal];

    /// The name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Self::Tabbed => "Sekmeli",
            Self::Compact => "Kompakt",
            Self::Flat => "Düz",
            Self::Minimal => "Minimal",
        }
    }
}

/// What the toolbar shows and how; stored with the preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolbarLayout {
    /// Tabs or one flat row.
    pub mode: ToolbarMode,
    /// Print each button's name next to its icon.
    pub captions: bool,
    /// Print the shortcut next to the name (needs `captions`).
    pub shortcuts: bool,
    /// Keep only the essential buttons.
    pub essentials: bool,
    /// Which categories show, in `ToolbarGroup::ALL` order.
    pub groups: [bool; 5],
}

impl Default for ToolbarLayout {
    fn default() -> Self {
        Self::preset(ToolbarPreset::Tabbed)
    }
}

impl ToolbarLayout {
    /// The layout of a preset.
    pub const fn preset(preset: ToolbarPreset) -> Self {
        let all = [true; 5];
        match preset {
            ToolbarPreset::Tabbed => Self {
                mode: ToolbarMode::Tabs,
                captions: true,
                shortcuts: true,
                essentials: false,
                groups: all,
            },
            ToolbarPreset::Compact => Self {
                mode: ToolbarMode::Tabs,
                captions: false,
                shortcuts: false,
                essentials: false,
                groups: all,
            },
            ToolbarPreset::Flat => Self {
                mode: ToolbarMode::Flat,
                captions: false,
                shortcuts: false,
                essentials: false,
                groups: all,
            },
            ToolbarPreset::Minimal => Self {
                mode: ToolbarMode::Flat,
                captions: false,
                shortcuts: false,
                essentials: true,
                groups: all,
            },
        }
    }

    /// The preset this layout equals, `None` once it was customised.
    pub fn preset_of(&self) -> Option<ToolbarPreset> {
        ToolbarPreset::ALL
            .into_iter()
            .find(|preset| Self::preset(*preset) == *self)
    }

    /// Whether `group` shows.
    pub fn group_shown(&self, group: ToolbarGroup) -> bool {
        ToolbarGroup::ALL
            .iter()
            .position(|candidate| *candidate == group)
            .is_some_and(|index| self.groups[index])
    }

    /// The first category that shows (the first of all when none is set).
    pub fn first_group(&self) -> ToolbarGroup {
        ToolbarGroup::ALL
            .into_iter()
            .find(|group| self.group_shown(*group))
            .unwrap_or(ToolbarGroup::Files)
    }

    /// Whether the button for `id` in `group` shows.
    pub fn shows(&self, id: CommandId, group: ToolbarGroup) -> bool {
        self.group_shown(group) && (!self.essentials || is_essential(id))
    }
}

/// The buttons the minimal toolbar keeps.
pub fn is_essential(id: CommandId) -> bool {
    matches!(
        id,
        C::Project
            | C::Files
            | C::Search
            | C::Save
            | C::SourceControl
            | C::Problems
            | C::NewTerminal
            | C::RunTask
            | C::Commands
            | C::Settings
    )
}

/// The name of the icon drawn on a toolbar button (a file of the UI's bundled toolbar icons).
pub fn toolbar_icon(id: CommandId) -> &'static str {
    match id {
        C::Project => "folder",
        C::Files => "file-search",
        C::Search => "text-search",
        C::Save => "save",
        C::SourceControl => "git",
        C::Problems => "alert",
        C::NewTerminal => "terminal-plus",
        C::Ssh => "network",
        C::Agent => "sparkle",
        C::RunTask => "play",
        C::RenameTerminal => "pencil",
        C::SplitTerminalRight => "split-right",
        C::SplitTerminalDown => "split-down",
        C::GrowTerminal => "expand",
        C::ShrinkTerminal => "shrink",
        C::StopTerminal => "stop",
        C::CloseTerminal => "close",
        C::NewTask => "task-plus",
        C::Handoff => "send",
        C::Memory => "book",
        C::Balanced => "layout-balanced",
        C::EditorLayout => "layout-editor",
        C::AgentsLayout => "layout-agents",
        C::SplitEditor => "columns",
        C::Inspector => "eye",
        C::ToggleTerminal => "panel-bottom",
        C::Commands => "command",
        C::Settings => "gear",
        _ => "dot",
    }
}

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

    #[test]
    fn presets_differ_and_are_found_again_until_customised() {
        let layouts: Vec<ToolbarLayout> = ToolbarPreset::ALL
            .into_iter()
            .map(ToolbarLayout::preset)
            .collect();
        for (index, layout) in layouts.iter().enumerate() {
            assert_eq!(layout.preset_of(), Some(ToolbarPreset::ALL[index]));
            assert!(layouts.iter().filter(|other| *other == layout).count() == 1);
        }
        let mut custom = ToolbarLayout::default();
        custom.groups[2] = false;
        assert_eq!(custom.preset_of(), None);
        assert!(!custom.group_shown(ToolbarGroup::Work));
        assert!(custom.group_shown(ToolbarGroup::Files));
    }

    #[test]
    fn the_minimal_preset_keeps_only_essential_buttons_that_exist() {
        let minimal = ToolbarLayout::preset(ToolbarPreset::Minimal);
        let mut group = ToolbarGroup::Files;
        let mut shown = 0;
        for item in toolbar() {
            match item {
                ToolbarItem::Group(next) => group = *next,
                ToolbarItem::Button(id, _) => shown += usize::from(minimal.shows(*id, group)),
            }
        }
        assert_eq!(shown, 10);
    }

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
        // Every group has at least one button, and the toolbar starts with a group.
        assert!(matches!(items.first(), Some(ToolbarItem::Group(_))));
        assert!(!matches!(items.last(), Some(ToolbarItem::Group(_))));
        assert!(items
            .windows(2)
            .all(|pair| !matches!(pair, [ToolbarItem::Group(_), ToolbarItem::Group(_)])));
        let mut ids: Vec<String> = items
            .iter()
            .filter_map(|item| match item {
                ToolbarItem::Button(id, _) => Some(format!("{id:?}")),
                ToolbarItem::Group(_) => None,
            })
            .collect();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn every_toolbar_button_has_its_own_icon() {
        let mut icons: Vec<&str> = toolbar()
            .iter()
            .filter_map(|item| match item {
                ToolbarItem::Button(id, _) => Some(toolbar_icon(*id)),
                ToolbarItem::Group(_) => None,
            })
            .collect();
        assert!(
            !icons.contains(&"dot"),
            "a button fell through to the default icon"
        );
        let before = icons.len();
        icons.sort_unstable();
        icons.dedup();
        assert_eq!(icons.len(), before, "two buttons share an icon");
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
