//! The command catalog of Workspace 0.1, ported one to one: one typed id per action, the
//! exact-modifier shortcut table, and the reasons a command is unavailable. Buttons, shortcuts
//! and the command palette all go through the same [`available`] check.

/// Every action the workbench offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    /// Quick command palette.
    Commands,
    /// Open a project folder.
    Project,
    /// Search files.
    Files,
    /// Save the open file.
    Save,
    /// Git diff of the open file.
    Diff,
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
    /// Clear the selected terminal's screen.
    ClearTerminal,
    /// Copy the terminal selection.
    CopyTerminal,
    /// Paste into the selected terminal.
    PasteTerminal,
    /// Stop the selected session.
    StopTerminal,
    /// Launch an agent.
    Agent,
    /// Create a handoff.
    Handoff,
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
    /// Switch between the standard and the GT-LCARS skin.
    Skin,
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

const COMMANDS: [Command; 31] = [
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
        CommandId::Files,
        "Dosya ara",
        Some("Ctrl P / Ctrl K"),
        &[bind("p"), bind("k")],
    ),
    cmd(
        CommandId::Save,
        "Dosyayı kaydet",
        Some("Ctrl S"),
        &[bind("s")],
    ),
    cmd(
        CommandId::Diff,
        "Açık dosyanın Git diff görünümü",
        None,
        &[],
    ),
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
        Some("Ctrl Shift I"),
        &[bind_terminal("i", true, false)],
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
    cmd(CommandId::Skin, "GT-LCARS görünümünü aç / kapat", None, &[]),
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
    /// Live terminal sessions (0 to 4).
    pub terminals: u8,
    /// The terminal dock is visible.
    pub dock_visible: bool,
    /// The agent and handoff features exist in this build (P4).
    pub control_plane_features: bool,
    /// The daemon answered its last request.
    pub daemon_reachable: bool,
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

/// Maximum simultaneous terminal sessions (spec section 7).
pub const MAX_TERMINALS: u8 = 4;

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
        Commands | Project | Settings | Skin | Shortcuts | Balanced | EditorLayout
        | AgentsLayout => allow(),
        Files | Refresh | FocusFiles | Memory | Inspector => needs_project(),
        Save => match (state.file_open, state.dirty) {
            (false, _) => deny("Açık dosya yok"),
            (true, false) => deny("Kaydedilecek değişiklik yok"),
            (true, true) => allow(),
        },
        Diff | FocusEditor => {
            if state.file_open {
                allow()
            } else {
                deny("Açık dosya yok")
            }
        }
        NewTerminal => {
            if !state.project_open {
                deny("Önce bir proje klasörü açın")
            } else if state.terminals >= MAX_TERMINALS {
                deny("En fazla 4 terminal açılabilir")
            } else {
                allow()
            }
        }
        ToggleTerminal => allow(),
        NextTerminal | PreviousTerminal | ClearTerminal | CopyTerminal | PasteTerminal
        | StopTerminal => needs_terminal(),
        FocusTerminal(index) => {
            if index >= 1 && index <= state.terminals {
                allow()
            } else {
                deny("Böyle bir terminal yok")
            }
        }
        Agent | Handoff | RegisterProject => {
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
        assert_eq!(catalog().len(), 31);
    }

    #[test]
    fn ctrl_shift_i_toggles_the_inspector_in_the_editor_and_in_a_terminal() {
        let mut stroke = stroke("i");
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
    fn the_fifth_terminal_is_refused_with_a_reason() {
        let state = WorkbenchState {
            project_open: true,
            terminals: 4,
            ..WorkbenchState::default()
        };
        let result = available(CommandId::NewTerminal, &state);
        assert!(!result.enabled);
        assert_eq!(result.reason, Some("En fazla 4 terminal açılabilir"));
        assert!(
            !available(
                CommandId::FocusTerminal(4),
                &WorkbenchState {
                    terminals: 3,
                    ..state
                }
            )
            .enabled
        );
        assert!(available(CommandId::FocusTerminal(4), &state).enabled);
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
