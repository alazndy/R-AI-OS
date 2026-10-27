//! The command catalog of Workspace 0.1, ported one to one: one typed id per action, the
//! exact-modifier shortcut table, and the reasons a command is unavailable. Buttons, shortcuts
//! and the command palette all go through the same [`available`] check.

use crate::sessions::MAX_SESSIONS;

/// Every action the workbench offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    /// Quick command palette.
    Commands,
    /// Open a project folder.
    Project,
    /// Choose one of the recently opened projects.
    RecentProjects,
    /// Search files.
    Files,
    /// Search the project's text through the RAIOS CLI's index.
    Search,
    /// Show the problems the language servers found.
    Problems,
    /// Format the open file with its language server.
    Format,
    /// Rename the symbol under the cursor in every file, through the language server.
    RenameSymbol,
    /// List where the symbol under the cursor is used.
    FindReferences,
    /// List the symbols of the open file.
    Outline,
    /// Search the symbols of the whole project through the language server.
    WorkspaceSymbols,
    /// Start the language servers again, also the ones that gave up.
    RestartServers,
    /// Show the next open file read-only beside the active one, or close that split.
    SplitEditor,
    /// Find text in the open file.
    FindInFile,
    /// Find and replace text in the open file.
    ReplaceInFile,
    /// Replace in the open file by regular expression (`$1` inserts a group).
    ReplaceRegexInFile,
    /// Ask for a line number and put the cursor there.
    GoToLine,
    /// Comment or uncomment the selected lines.
    ToggleComment,
    /// Wrap long lines at the window edge, or stop wrapping.
    ToggleWordWrap,
    /// Make the editor and terminal text bigger.
    ZoomIn,
    /// Make the editor and terminal text smaller.
    ZoomOut,
    /// Back to the default text size.
    ZoomReset,
    /// Save changed files when the window loses focus (on or off).
    ToggleAutoSave,
    /// Format with the language server before saving (on or off).
    ToggleFormatOnSave,
    /// Run a task from the user's task file in a new terminal.
    RunTask,
    /// Open the file and line that the selected terminal's output points at.
    OpenOutputLocation,
    /// Show the repository's changes: stage, commit, switch branch.
    SourceControl,
    /// Save the open file.
    Save,
    /// Close the active editor tab.
    CloseTab,
    /// Git diff of the open file.
    Diff,
    /// Switch the diff between unified and side-by-side.
    DiffSplit,
    /// New terminal or split session.
    NewTerminal,
    /// Show or hide the terminal dock.
    ToggleTerminal,
    /// Focus the editor.
    FocusEditor,
    /// Focus the file tree.
    FocusFiles,
    /// Select the next terminal.
    NextTerminal,
    /// Select the previous terminal.
    PreviousTerminal,
    /// Focus terminal 1 to 4.
    FocusTerminal(u8),
    /// Show the buttons of toolbar category 1 to 5.
    ToolbarTab(u8),
    /// Clear the selected terminal's screen.
    ClearTerminal,
    /// Search the selected terminal's output, scrollback included.
    FindInTerminal,
    /// Copy the terminal selection.
    CopyTerminal,
    /// Paste into the selected terminal.
    PasteTerminal,
    /// Stop the selected session; its pane stays so the output can still be read.
    StopTerminal,
    /// Stop the selected session and remove its pane.
    CloseTerminal,
    /// End the selected session at once (SIGKILL, no grace for an agent) and remove its pane.
    KillTerminal,
    /// Start the selected SSH shell again after its connection ended.
    Reconnect,
    /// Open a remote shell over SSH.
    Ssh,
    /// Give the selected terminal a name.
    RenameTerminal,
    /// Swap the selected terminal with the one before it in the dock.
    MoveTerminalPrevious,
    /// Swap the selected terminal with the one after it in the dock.
    MoveTerminalNext,
    /// Place the selected terminal to the right of its neighbour.
    SplitTerminalRight,
    /// Place the selected terminal below its neighbour.
    SplitTerminalDown,
    /// Make the selected terminal a tab of the group before it.
    JoinTerminalPrevious,
    /// Take the selected terminal out of its tab group.
    UngroupTerminal,
    /// Give the selected terminal more room.
    GrowTerminal,
    /// Give the selected terminal less room.
    ShrinkTerminal,
    /// Launch an agent.
    Agent,
    /// Create a handoff.
    Handoff,
    /// Add an operator task to the open project.
    NewTask,
    /// Show the project memory.
    Memory,
    /// Register the open project with Raios.
    RegisterProject,
    /// Show or hide the inspector (tasks, runs and approvals of the project).
    Inspector,
    /// Refresh the file tree and RAIOS data.
    Refresh,
    /// Appearance settings.
    Settings,
    /// Keyboard shortcut list.
    Shortcuts,
    /// Switch to the next skin (standard, GT-LCARS, Aperture).
    Skin,
    /// Widen the dock (the divider moves left).
    GrowDock,
    /// Narrow the dock (the divider moves right).
    ShrinkDock,
    /// Balanced layout.
    Balanced,
    /// Editor-first layout.
    EditorLayout,
    /// Terminal and agent layout.
    AgentsLayout,
}

/// One key combination. Every binding needs Ctrl (or the platform key); `shift` and `alt` must
/// match exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// Layout-independent key name as GPUI reports it (`p`, `1`, `` ` ``, `left`, `right`).
    pub key: &'static str,
    /// Shift must be held.
    pub shift: bool,
    /// Alt must be held.
    pub alt: bool,
    /// The binding stays active while a terminal has focus.
    pub terminal: bool,
}

const fn bind(key: &'static str) -> Binding {
    Binding {
        key,
        shift: false,
        alt: false,
        terminal: false,
    }
}

const fn bind_terminal(key: &'static str, shift: bool, alt: bool) -> Binding {
    Binding {
        key,
        shift,
        alt,
        terminal: true,
    }
}

/// A command with its label and key bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// The action.
    pub id: CommandId,
    /// Turkish label, as in Workspace 0.1.
    pub label: &'static str,
    /// Shortcut text shown next to the label.
    pub shortcut: Option<&'static str>,
    /// Key combinations that run the command.
    pub bindings: &'static [Binding],
}

const fn cmd(
    id: CommandId,
    label: &'static str,
    shortcut: Option<&'static str>,
    bindings: &'static [Binding],
) -> Command {
    Command {
        id,
        label,
        shortcut,
        bindings,
    }
}

const COMMANDS: [Command; 79] = [
    cmd(
        CommandId::Commands,
        "Hızlı komutlar",
        Some("Ctrl Shift P"),
        &[bind_terminal("p", true, false)],
    ),
    cmd(
        CommandId::Project,
        "Proje klasörü aç",
        Some("Ctrl O"),
        &[bind("o")],
    ),
    cmd(
        CommandId::RecentProjects,
        "Son açılan projeler",
        Some("Ctrl Alt O"),
        &[bind_terminal("o", false, true)],
    ),
    cmd(
        CommandId::Files,
        "Dosya ara",
        Some("Ctrl P / Ctrl K"),
        &[bind("p"), bind("k")],
    ),
    cmd(
        CommandId::Search,
        "Projede ara",
        Some("Ctrl Shift F"),
        &[bind_terminal("f", true, false)],
    ),
    cmd(
        CommandId::Save,
        "Dosyayı kaydet",
        Some("Ctrl S"),
        &[bind("s")],
    ),
    cmd(
        CommandId::CloseTab,
        "Sekmeyi kapat",
        Some("Ctrl W"),
        &[bind("w")],
    ),
    cmd(
        CommandId::Diff,
        "Açık dosyanın Git diff görünümü",
        None,
        &[],
    ),
    cmd(CommandId::DiffSplit, "Diff: yan yana / birleşik", None, &[]),
    cmd(
        CommandId::NewTerminal,
        "Yeni terminal / bölünmüş oturum",
        Some("Ctrl Shift T"),
        &[
            bind_terminal("t", true, false),
            bind_terminal("`", true, false),
        ],
    ),
    cmd(
        CommandId::ToggleTerminal,
        "Terminali göster / gizle",
        Some("Ctrl `"),
        &[bind_terminal("`", false, false)],
    ),
    cmd(
        CommandId::FocusEditor,
        "Editöre odaklan",
        Some("Ctrl Shift J"),
        &[bind_terminal("j", true, false)],
    ),
    cmd(
        CommandId::FocusFiles,
        "Dosya ağacına odaklan",
        Some("Ctrl Shift E"),
        &[bind_terminal("e", true, false)],
    ),
    cmd(
        CommandId::NextTerminal,
        "Sonraki terminal",
        Some("Ctrl Alt →"),
        &[bind_terminal("right", false, true)],
    ),
    cmd(
        CommandId::PreviousTerminal,
        "Önceki terminal",
        Some("Ctrl Alt ←"),
        &[bind_terminal("left", false, true)],
    ),
    cmd(
        CommandId::FocusTerminal(1),
        "1. terminale odaklan",
        Some("Ctrl Alt 1"),
        &[bind_terminal("1", false, true)],
    ),
    cmd(
        CommandId::FocusTerminal(2),
        "2. terminale odaklan",
        Some("Ctrl Alt 2"),
        &[bind_terminal("2", false, true)],
    ),
    cmd(
        CommandId::FocusTerminal(3),
        "3. terminale odaklan",
        Some("Ctrl Alt 3"),
        &[bind_terminal("3", false, true)],
    ),
    cmd(
        CommandId::FocusTerminal(4),
        "4. terminale odaklan",
        Some("Ctrl Alt 4"),
        &[bind_terminal("4", false, true)],
    ),
    cmd(
        CommandId::ToolbarTab(1),
        "Araç çubuğu: Dosya sekmesi",
        Some("Ctrl Shift 1"),
        &[bind_terminal("1", true, false)],
    ),
    cmd(
        CommandId::ToolbarTab(2),
        "Araç çubuğu: Terminal sekmesi",
        Some("Ctrl Shift 2"),
        &[bind_terminal("2", true, false)],
    ),
    cmd(
        CommandId::ToolbarTab(3),
        "Araç çubuğu: İş sekmesi",
        Some("Ctrl Shift 3"),
        &[bind_terminal("3", true, false)],
    ),
    cmd(
        CommandId::ToolbarTab(4),
        "Araç çubuğu: Görünüm sekmesi",
        Some("Ctrl Shift 4"),
        &[bind_terminal("4", true, false)],
    ),
    cmd(
        CommandId::ToolbarTab(5),
        "Araç çubuğu: Araç sekmesi",
        Some("Ctrl Shift 5"),
        &[bind_terminal("5", true, false)],
    ),
    cmd(
        CommandId::FindInTerminal,
        "Terminal çıktısında ara",
        Some("Ctrl Alt F"),
        &[bind_terminal("f", false, true)],
    ),
    cmd(
        CommandId::ClearTerminal,
        "Seçili terminalin görüntüsünü temizle",
        None,
        &[],
    ),
    cmd(
        CommandId::CopyTerminal,
        "Terminal seçimini kopyala",
        Some("Ctrl Shift C"),
        &[bind_terminal("c", true, false)],
    ),
    cmd(
        CommandId::PasteTerminal,
        "Seçili terminale yapıştır",
        Some("Ctrl Shift V"),
        &[bind_terminal("v", true, false)],
    ),
    cmd(CommandId::StopTerminal, "Seçili oturumu durdur…", None, &[]),
    cmd(
        CommandId::CloseTerminal,
        "Terminali kapat",
        Some("Ctrl Shift W"),
        &[bind_terminal("w", true, false)],
    ),
    cmd(CommandId::KillTerminal, "Terminali zorla öldür…", None, &[]),
    cmd(CommandId::Reconnect, "SSH'e yeniden bağlan", None, &[]),
    cmd(
        CommandId::Ssh,
        "SSH ile bağlan…",
        Some("Ctrl Shift S"),
        &[bind_terminal("s", true, false)],
    ),
    cmd(
        CommandId::RenameTerminal,
        "Terminali yeniden adlandır…",
        None,
        &[],
    ),
    cmd(
        CommandId::MoveTerminalPrevious,
        "Terminali öncekiyle yer değiştir",
        None,
        &[],
    ),
    cmd(
        CommandId::MoveTerminalNext,
        "Terminali sonrakiyle yer değiştir",
        None,
        &[],
    ),
    cmd(
        CommandId::SplitTerminalRight,
        "Terminali komşusunun sağına böl",
        None,
        &[],
    ),
    cmd(
        CommandId::SplitTerminalDown,
        "Terminali komşusunun altına böl",
        None,
        &[],
    ),
    cmd(
        CommandId::JoinTerminalPrevious,
        "Terminali öncekinin sekmesi yap",
        None,
        &[],
    ),
    cmd(
        CommandId::UngroupTerminal,
        "Terminali sekme grubundan ayır",
        None,
        &[],
    ),
    cmd(CommandId::GrowTerminal, "Terminali büyüt", None, &[]),
    cmd(CommandId::ShrinkTerminal, "Terminali küçült", None, &[]),
    cmd(
        CommandId::Agent,
        "Ajan başlat",
        Some("Ctrl Shift A"),
        &[bind_terminal("a", true, false)],
    ),
    cmd(
        CommandId::Handoff,
        "Handoff oluştur",
        Some("Ctrl Shift H"),
        &[bind_terminal("h", true, false)],
    ),
    cmd(CommandId::NewTask, "Yeni görev…", None, &[]),
    cmd(CommandId::RunTask, "Görev çalıştır", None, &[]),
    cmd(
        CommandId::OpenOutputLocation,
        "Çıktıdaki konumu aç",
        None,
        &[],
    ),
    cmd(CommandId::SourceControl, "Git değişiklikleri", None, &[]),
    cmd(CommandId::SplitEditor, "Editörü böl / birleştir", None, &[]),
    cmd(CommandId::FindInFile, "Dosyada bul", Some("Ctrl F"), &[]),
    cmd(
        CommandId::ReplaceInFile,
        "Dosyada bul ve değiştir",
        Some("Ctrl H"),
        &[],
    ),
    cmd(
        CommandId::ReplaceRegexInFile,
        "Dosyada regex ile değiştir",
        None,
        &[],
    ),
    cmd(
        CommandId::GoToLine,
        "Satıra git",
        Some("Ctrl G"),
        &[bind("g")],
    ),
    cmd(
        CommandId::ToggleComment,
        "Satırı yorumla / yorumu kaldır",
        Some("Ctrl /"),
        &[bind("/")],
    ),
    cmd(
        CommandId::ToggleWordWrap,
        "Satır kaydırmayı aç/kapat",
        None,
        &[],
    ),
    cmd(
        CommandId::ZoomIn,
        "Yazıyı büyüt",
        Some("Ctrl +"),
        &[
            bind_terminal("=", false, false),
            bind_terminal("=", true, false),
        ],
    ),
    cmd(
        CommandId::ZoomOut,
        "Yazıyı küçült",
        Some("Ctrl -"),
        &[bind_terminal("-", false, false)],
    ),
    cmd(
        CommandId::ZoomReset,
        "Yazı boyutunu sıfırla",
        Some("Ctrl 0"),
        &[bind_terminal("0", false, false)],
    ),
    cmd(
        CommandId::ToggleAutoSave,
        "Otomatik kaydetmeyi aç/kapat",
        None,
        &[],
    ),
    cmd(
        CommandId::ToggleFormatOnSave,
        "Kaydederken biçimlendirmeyi aç/kapat",
        None,
        &[],
    ),
    cmd(CommandId::Format, "Dosyayı biçimlendir", None, &[]),
    cmd(
        CommandId::RenameSymbol,
        "Sembolü yeniden adlandır",
        None,
        &[],
    ),
    cmd(
        CommandId::FindReferences,
        "Sembolün kullanıldığı yerleri bul",
        None,
        &[],
    ),
    cmd(CommandId::Outline, "Dosyadaki sembolleri göster", None, &[]),
    cmd(
        CommandId::WorkspaceSymbols,
        "Projede sembol ara",
        Some("Ctrl T"),
        &[bind("t")],
    ),
    cmd(
        CommandId::RestartServers,
        "Dil sunucularını yeniden başlat",
        None,
        &[],
    ),
    cmd(CommandId::Problems, "Sorunları göster", None, &[]),
    cmd(CommandId::Memory, "Proje hafızasını göster", None, &[]),
    cmd(
        CommandId::RegisterProject,
        "Projeyi Raios'a kaydet",
        None,
        &[],
    ),
    cmd(
        CommandId::Inspector,
        "Denetçiyi göster / gizle",
        Some("Ctrl Shift D"),
        &[bind_terminal("d", true, false)],
    ),
    cmd(
        CommandId::Refresh,
        "Dosya ağacı ve Raios verisini yenile",
        None,
        &[],
    ),
    cmd(
        CommandId::Settings,
        "Görünüm ayarları",
        Some("Ctrl ,"),
        &[bind(",")],
    ),
    cmd(
        CommandId::Skin,
        "Görünümü değiştir (standart / GT-LCARS / Aperture)",
        None,
        &[],
    ),
    cmd(
        CommandId::GrowDock,
        "Dock'u genişlet",
        Some("Ctrl Alt ."),
        &[bind_terminal(".", false, true)],
    ),
    cmd(
        CommandId::ShrinkDock,
        "Dock'u daralt",
        Some("Ctrl Alt ,"),
        &[bind_terminal(",", false, true)],
    ),
    cmd(CommandId::Shortcuts, "Klavye kısayolları", None, &[]),
    cmd(
        CommandId::Balanced,
        "Dengeli yerleşim",
        Some("Ctrl 1"),
        &[bind("1")],
    ),
    cmd(
        CommandId::EditorLayout,
        "Editör yerleşimi",
        Some("Ctrl 2"),
        &[bind("2")],
    ),
    cmd(
        CommandId::AgentsLayout,
        "Terminal ve ajan yerleşimi",
        Some("Ctrl 3"),
        &[bind("3")],
    ),
];

/// The whole catalog in palette order.
pub fn catalog() -> &'static [Command] {
    &COMMANDS
}

/// Where keyboard focus is, which decides which bindings apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyboardContext {
    /// A terminal has focus: only bindings marked `terminal` apply, so Ctrl+C, Ctrl+K and the
    /// rest reach the shell.
    pub terminal: bool,
    /// A modal dialog is open: no shortcut applies.
    pub dialog: bool,
    /// A text input (palette query, search field) has focus: only the palette shortcut applies.
    pub input: bool,
}

/// One key press as the resolver needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keystroke {
    /// Layout-independent key name.
    pub key: String,
    /// Control is held.
    pub ctrl: bool,
    /// The platform (super) key is held.
    pub platform: bool,
    /// Alt is held.
    pub alt: bool,
    /// Shift is held.
    pub shift: bool,
    /// An IME composition is in progress.
    pub composing: bool,
    /// The key is auto-repeating.
    pub repeat: bool,
    /// AltGr produced this key (a character, not a shortcut).
    pub alt_graph: bool,
}

/// The command a key press runs, or `None` when it belongs to the focused widget. Held keys do
/// not repeat commands unless `consume_repeat` is set.
pub fn resolve_shortcut(
    stroke: &Keystroke,
    context: KeyboardContext,
    consume_repeat: bool,
) -> Option<CommandId> {
    if (stroke.repeat && !consume_repeat)
        || stroke.composing
        || stroke.alt_graph
        || context.dialog
        || !(stroke.ctrl || stroke.platform)
        || (stroke.ctrl && stroke.platform)
    {
        return None;
    }
    let key = stroke.key.to_lowercase();
    for command in &COMMANDS {
        for binding in command.bindings {
            if matches!(
                command.id,
                CommandId::CopyTerminal | CommandId::PasteTerminal
            ) && !context.terminal
            {
                continue;
            }
            if binding.shift != stroke.shift || binding.alt != stroke.alt {
                continue;
            }
            if context.terminal && !binding.terminal {
                continue;
            }
            if context.input && !context.terminal && command.id != CommandId::Commands {
                continue;
            }
            if binding.key == key {
                return Some(command.id);
            }
        }
    }
    None
}

/// What the workbench knows when it decides whether a command can run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkbenchState {
    /// A project folder is open.
    pub project_open: bool,
    /// A file is open in the editor.
    pub file_open: bool,
    /// The open file has unsaved edits.
    pub dirty: bool,
    /// Live terminal sessions (0 to [`MAX_SESSIONS`]).
    pub terminals: u8,
    /// The terminal dock is visible.
    pub dock_visible: bool,
    /// The agent and handoff features exist in this build (P4).
    pub control_plane_features: bool,
    /// The daemon answered its last request.
    pub daemon_reachable: bool,
    /// The selected terminal is an SSH shell whose connection has ended.
    pub reconnectable: bool,
}

/// Why a command can or cannot run right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandState {
    /// The command can run.
    pub enabled: bool,
    /// The reason it cannot, shown in the palette.
    pub reason: Option<&'static str>,
}

fn allow() -> CommandState {
    CommandState {
        enabled: true,
        reason: None,
    }
}

fn deny(reason: &'static str) -> CommandState {
    CommandState {
        enabled: false,
        reason: Some(reason),
    }
}

/// Whether `id` can run in `state`, with the reason when it cannot.
pub fn available(id: CommandId, state: &WorkbenchState) -> CommandState {
    use CommandId::*;
    let needs_project = || {
        if state.project_open {
            allow()
        } else {
            deny("Önce bir proje klasörü açın")
        }
    };
    let needs_terminal = || {
        if state.terminals > 0 {
            allow()
        } else {
            deny("Açık terminal yok")
        }
    };
    match id {
        ToggleAutoSave | ToggleFormatOnSave | ToggleWordWrap | ZoomIn | ZoomOut | ZoomReset
        | Commands | Project | RecentProjects | Settings | Skin | Shortcuts | Balanced
        | EditorLayout | AgentsLayout => allow(),
        Files | Search | Problems | SourceControl | Refresh | RunTask | FocusFiles | Memory
        | Inspector | RestartServers => needs_project(),
        Save => match (state.file_open, state.dirty) {
            (false, _) => deny("Açık dosya yok"),
            (true, false) => deny("Kaydedilecek değişiklik yok"),
            (true, true) => allow(),
        },
        Diff | DiffSplit | FocusEditor | CloseTab | Format | RenameSymbol | FindReferences
        | Outline | WorkspaceSymbols | FindInFile | ReplaceInFile | ReplaceRegexInFile
        | GoToLine | ToggleComment | SplitEditor => {
            if state.file_open {
                allow()
            } else {
                deny("Açık dosya yok")
            }
        }
        NewTerminal | Ssh => {
            if !state.project_open {
                deny("Önce bir proje klasörü açın")
            } else if usize::from(state.terminals) >= MAX_SESSIONS {
                deny("En fazla 12 terminal açılabilir")
            } else {
                allow()
            }
        }
        Reconnect => {
            if state.reconnectable {
                allow()
            } else {
                deny("Seçili terminal bitmiş bir SSH oturumu değil")
            }
        }
        ToggleTerminal => allow(),
        GrowDock | ShrinkDock => {
            if state.dock_visible {
                allow()
            } else {
                deny("Dock gizli")
            }
        }
        NextTerminal | PreviousTerminal | ClearTerminal | FindInTerminal | CopyTerminal
        | PasteTerminal | OpenOutputLocation | StopTerminal | CloseTerminal | KillTerminal
        | RenameTerminal | UngroupTerminal => needs_terminal(),
        MoveTerminalPrevious | MoveTerminalNext | SplitTerminalRight | SplitTerminalDown
        | JoinTerminalPrevious | GrowTerminal | ShrinkTerminal => {
            if state.terminals >= 2 {
                allow()
            } else {
                deny("En az iki terminal gerekir")
            }
        }
        ToolbarTab(number) => {
            if (1..=5).contains(&number) {
                allow()
            } else {
                deny("Böyle bir sekme yok")
            }
        }
        FocusTerminal(index) => {
            if index >= 1 && index <= state.terminals {
                allow()
            } else {
                deny("Böyle bir terminal yok")
            }
        }
        Agent | Handoff | NewTask | RegisterProject => {
            if !state.control_plane_features {
                deny("Daemon bağlantısı yapılandırılmadı")
            } else if !state.project_open {
                deny("Önce bir proje klasörü açın")
            } else if !state.daemon_reachable {
                deny("Daemon'a ulaşılamıyor")
            } else {
                allow()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(key: &str) -> Keystroke {
        Keystroke {
            key: key.into(),
            ctrl: true,
            platform: false,
            alt: false,
            shift: false,
            composing: false,
            repeat: false,
            alt_graph: false,
        }
    }

    const EDITOR: KeyboardContext = KeyboardContext {
        terminal: false,
        dialog: false,
        input: false,
    };
    const TERMINAL: KeyboardContext = KeyboardContext {
        terminal: true,
        dialog: false,
        input: false,
    };

    #[test]
    fn the_catalog_has_unique_ids_and_labels() {
        let mut labels: Vec<&str> = catalog().iter().map(|c| c.label).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), catalog().len());
        assert_eq!(catalog().len(), 79);
    }

    #[test]
    fn ctrl_shift_d_toggles_the_inspector_in_the_editor_and_in_a_terminal() {
        // Ctrl+Shift+I is not usable: a Turkish layout reports it as a different key.
        let mut stroke = stroke("d");
        stroke.shift = true;
        assert_eq!(
            resolve_shortcut(&stroke, EDITOR, false),
            Some(CommandId::Inspector)
        );
        assert_eq!(
            resolve_shortcut(&stroke, TERMINAL, false),
            Some(CommandId::Inspector)
        );
    }

    #[test]
    fn ctrl_shift_f_searches_the_project_from_every_pane() {
        let mut stroke = stroke("f");
        stroke.shift = true;
        assert_eq!(
            resolve_shortcut(&stroke, EDITOR, false),
            Some(CommandId::Search)
        );
        assert_eq!(
            resolve_shortcut(&stroke, TERMINAL, false),
            Some(CommandId::Search)
        );
    }

    #[test]
    fn search_needs_a_project() {
        let closed = WorkbenchState::default();
        assert_eq!(
            available(CommandId::Search, &closed).reason,
            Some("Önce bir proje klasörü açın")
        );
        let open = WorkbenchState {
            project_open: true,
            ..WorkbenchState::default()
        };
        assert!(available(CommandId::Search, &open).enabled);
    }

    #[test]
    fn plain_ctrl_keys_reach_the_shell_when_a_terminal_has_focus() {
        for key in ["c", "k", "s", "l", "p", "o", "1"] {
            assert_eq!(
                resolve_shortcut(&stroke(key), TERMINAL, false),
                None,
                "{key}"
            );
        }
    }

    #[test]
    fn terminal_bindings_still_work_inside_a_terminal() {
        let mut s = stroke("t");
        s.shift = true;
        assert_eq!(
            resolve_shortcut(&s, TERMINAL, false),
            Some(CommandId::NewTerminal)
        );
        let mut s = stroke("`");
        assert_eq!(
            resolve_shortcut(&s, TERMINAL, false),
            Some(CommandId::ToggleTerminal)
        );
        s.shift = true;
        assert_eq!(
            resolve_shortcut(&s, TERMINAL, false),
            Some(CommandId::NewTerminal)
        );
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let mut s = stroke("p");
        assert_eq!(resolve_shortcut(&s, EDITOR, false), Some(CommandId::Files));
        s.shift = true;
        assert_eq!(
            resolve_shortcut(&s, EDITOR, false),
            Some(CommandId::Commands)
        );
        s.alt = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
    }

    #[test]
    fn copy_and_paste_commands_exist_only_in_a_terminal() {
        let mut c = stroke("c");
        c.shift = true;
        assert_eq!(resolve_shortcut(&c, EDITOR, false), None);
        assert_eq!(
            resolve_shortcut(&c, TERMINAL, false),
            Some(CommandId::CopyTerminal)
        );
    }

    #[test]
    fn composition_altgr_dialogs_and_repeats_never_run_commands() {
        let mut s = stroke("s");
        s.composing = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
        let mut s = stroke("s");
        s.alt_graph = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
        let s = stroke("s");
        let dialog = KeyboardContext {
            dialog: true,
            ..EDITOR
        };
        assert_eq!(resolve_shortcut(&s, dialog, false), None);
        let mut s = stroke("s");
        s.repeat = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
        assert_eq!(resolve_shortcut(&s, EDITOR, true), Some(CommandId::Save));
    }

    #[test]
    fn ctrl_and_the_platform_key_together_are_not_a_shortcut() {
        let mut s = stroke("s");
        s.platform = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
        let mut s = stroke("s");
        s.ctrl = false;
        s.platform = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), Some(CommandId::Save));
        let mut s = stroke("s");
        s.ctrl = false;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
    }

    #[test]
    fn a_text_input_only_lets_the_palette_shortcut_through() {
        let input = KeyboardContext {
            input: true,
            ..EDITOR
        };
        assert_eq!(resolve_shortcut(&stroke("s"), input, false), None);
        let mut p = stroke("p");
        p.shift = true;
        assert_eq!(
            resolve_shortcut(&p, input, false),
            Some(CommandId::Commands)
        );
    }

    #[test]
    fn save_explains_why_it_is_unavailable() {
        let mut state = WorkbenchState::default();
        assert_eq!(
            available(CommandId::Save, &state).reason,
            Some("Açık dosya yok")
        );
        state.file_open = true;
        assert_eq!(
            available(CommandId::Save, &state).reason,
            Some("Kaydedilecek değişiklik yok")
        );
        state.dirty = true;
        assert!(available(CommandId::Save, &state).enabled);
    }

    #[test]
    fn the_thirteenth_terminal_is_refused_with_a_reason() {
        let state = WorkbenchState {
            project_open: true,
            terminals: 12,
            ..WorkbenchState::default()
        };
        let result = available(CommandId::NewTerminal, &state);
        assert!(!result.enabled);
        assert_eq!(result.reason, Some("En fazla 12 terminal açılabilir"));
        assert!(
            !available(
                CommandId::FocusTerminal(12),
                &WorkbenchState {
                    terminals: 11,
                    ..state
                }
            )
            .enabled
        );
        assert!(available(CommandId::FocusTerminal(12), &state).enabled);
    }

    #[test]
    fn agent_commands_fail_closed_without_the_feature_or_the_daemon() {
        let mut state = WorkbenchState {
            project_open: true,
            ..WorkbenchState::default()
        };
        assert_eq!(
            available(CommandId::Agent, &state).reason,
            Some("Daemon bağlantısı yapılandırılmadı")
        );
        state.control_plane_features = true;
        assert_eq!(
            available(CommandId::Handoff, &state).reason,
            Some("Daemon'a ulaşılamıyor")
        );
        state.daemon_reachable = true;
        assert!(available(CommandId::Handoff, &state).enabled);
    }
}
