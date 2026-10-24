//! Context menus and the toolbar of the workbench.
//!
//! A right click first selects what it hit (a terminal, a file, a tab, a task) and then opens a
//! menu whose lines are catalog commands, so each line is enabled, labelled and described by
//! the same rules as the palette, the shortcuts and the toolbar. The few lines that are not
//! catalog commands (open a file, copy its path, change a task) are listed here.

use gpui::{
    div, px, AnyElement, ClipboardItem, Context, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Pixels, Point, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window,
};
use gpui::{AppContext, Entity};
use rwk_core::handoff_flow::TaskLink;
use rwk_core::tasks::TaskStatus;
use rwk_core::workbench::{
    available, catalog, menu_entries, toolbar, toolbar_pressed, CommandId, MenuEntry, MenuTarget,
    Theme, ToolbarItem,
};

use super::Workbench;
use crate::context_menu::{ContextMenu, ContextMenuEvent, Line};

/// What choosing a menu line does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// Run a catalog command.
    Command(CommandId),
    /// Open a file of the tree (relative path).
    OpenFile(String),
    /// Put a path on the clipboard.
    CopyPath(String),
    /// Set the status of a task.
    TaskStatus {
        /// Task identifier.
        task_id: String,
        /// The new status.
        status: TaskStatus,
    },
    /// Give a task to the agent of the selected terminal.
    TaskAssign {
        /// Task identifier.
        task_id: String,
        /// Revision the row was displayed at.
        updated_at: String,
    },
    /// Clear the assignee of a task.
    TaskClear {
        /// Task identifier.
        task_id: String,
        /// Revision the row was displayed at.
        updated_at: String,
    },
    /// Hand a task over through the handoff composer.
    TaskHandoff {
        /// Task identifier.
        task_id: String,
        /// Title, seeded into the note.
        title: String,
    },
}

/// What a task row tells the menu opened on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMenuInfo {
    /// Task identifier.
    pub task_id: String,
    /// Title.
    pub title: String,
    /// Status label from the daemon.
    pub status: String,
    /// The task has an assignee.
    pub assigned: bool,
    /// Revision the row was displayed at.
    pub updated_at: String,
}

/// The menu that is open.
pub(super) struct OpenMenu {
    pub(super) view: Entity<ContextMenu>,
    /// The action of each line, in the order of the lines; rules and disabled lines have none.
    actions: Vec<Option<MenuAction>>,
    /// Where the keyboard was before the menu opened.
    restore: Option<FocusHandle>,
    _subscription: Subscription,
}

/// A tooltip: one line of text.
struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .text_xs()
            .rounded_sm()
            .bg(gpui::rgba(0x000000dd))
            .text_color(gpui::rgb(0xffffff))
            .child(self.0.clone())
    }
}

impl Workbench {
    /// The open context menu.
    pub fn context_menu(&self) -> Option<&Entity<ContextMenu>> {
        self.menu.as_ref().map(|open| &open.view)
    }

    /// The action behind line `index` of the open menu.
    pub fn menu_action(&self, index: usize) -> Option<MenuAction> {
        self.menu
            .as_ref()
            .and_then(|open| open.actions.get(index).cloned().flatten())
    }

    /// Lines for the commands of a static menu, resolved against what is available now.
    fn command_lines(
        &self,
        entries: &[MenuEntry],
        cx: &gpui::App,
    ) -> Vec<(Line, Option<MenuAction>)> {
        let state = self.state(cx);
        entries
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Separator => Some((Line::Rule, None)),
                MenuEntry::Command(id) => {
                    let command = catalog().iter().find(|command| command.id == *id)?;
                    let verdict = available(*id, &state);
                    Some(if verdict.enabled {
                        (
                            Line::item(command.label, command.shortcut),
                            Some(MenuAction::Command(*id)),
                        )
                    } else {
                        (Line::disabled(command.label, verdict.reason), None)
                    })
                }
            })
            .collect()
    }

    /// Opens the menu of `target` at `at`. A right click selects what it hit before this is
    /// called, so the commands act on it.
    pub fn open_context_menu(
        &mut self,
        target: MenuTarget,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lines = self.command_lines(menu_entries(target), cx);
        self.open_menu(lines, at, window, cx);
    }

    /// Opens the menu of a file-tree row.
    pub fn open_file_menu(
        &mut self,
        path: String,
        folder: bool,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut lines = Vec::new();
        if !folder {
            lines.push((
                Line::item("Aç", None),
                Some(MenuAction::OpenFile(path.clone())),
            ));
        }
        lines.push((
            Line::item("Yolu kopyala", None),
            Some(MenuAction::CopyPath(path)),
        ));
        lines.push((Line::Rule, None));
        lines.extend(self.command_lines(
            &[
                MenuEntry::Command(CommandId::Files),
                MenuEntry::Command(CommandId::Search),
                MenuEntry::Command(CommandId::Refresh),
            ],
            cx,
        ));
        self.open_menu(lines, at, window, cx);
    }

    /// Opens the menu of a task row.
    pub fn open_task_menu(
        &mut self,
        task: TaskMenuInfo,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut lines: Vec<(Line, Option<MenuAction>)> = Vec::new();
        for (label, status) in [
            ("Başlat", TaskStatus::InProgress),
            ("Bitir", TaskStatus::Completed),
            ("İptal et", TaskStatus::Cancelled),
        ] {
            if task.status != status.as_str() {
                lines.push((
                    Line::item(label, None),
                    Some(MenuAction::TaskStatus {
                        task_id: task.task_id.clone(),
                        status,
                    }),
                ));
            }
        }
        lines.push((Line::Rule, None));
        let agent_selected = self
            .terminals
            .get(self.selected)
            .is_some_and(|pane| pane.kind.agent().is_some());
        lines.push(if agent_selected {
            (
                Line::item("Seçili terminalin ajanına ata", None),
                Some(MenuAction::TaskAssign {
                    task_id: task.task_id.clone(),
                    updated_at: task.updated_at.clone(),
                }),
            )
        } else {
            (
                Line::disabled(
                    "Seçili terminalin ajanına ata",
                    Some("Ajan terminali seçin"),
                ),
                None,
            )
        });
        lines.push((
            Line::item("Devret…", None),
            Some(MenuAction::TaskHandoff {
                task_id: task.task_id.clone(),
                title: task.title.clone(),
            }),
        ));
        if task.assigned {
            lines.push((
                Line::item("Atamayı kaldır", None),
                Some(MenuAction::TaskClear {
                    task_id: task.task_id,
                    updated_at: task.updated_at,
                }),
            ));
        }
        self.open_menu(lines, at, window, cx);
    }

    fn open_menu(
        &mut self,
        lines: Vec<(Line, Option<MenuAction>)>,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if lines.is_empty() {
            return;
        }
        // Only one menu at a time, and not on top of a dialog that is waiting for an answer.
        if self.pending.is_some() {
            return;
        }
        self.dismiss_menu(false, window, cx);
        let theme = self.theme();
        let (lines, actions): (Vec<Line>, Vec<Option<MenuAction>>) = lines.into_iter().unzip();
        let view = cx.new(|_| ContextMenu::new(lines, at));
        view.update(cx, |menu, cx| menu.set_theme(theme, cx));
        let subscription = cx.subscribe_in(
            &view,
            window,
            |this, _menu, event: &ContextMenuEvent, window, cx| match *event {
                ContextMenuEvent::Close => this.close_context_menu(window, cx),
                ContextMenuEvent::Chosen(index) => {
                    let action = this.menu_action(index);
                    this.close_context_menu(window, cx);
                    if let Some(action) = action {
                        this.run_menu_action(action, window, cx);
                    }
                }
            },
        );
        self.menu = Some(OpenMenu {
            view,
            actions,
            restore: window.focused(cx),
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Closes the menu and gives the keyboard back to where it was.
    pub fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_menu(true, window, cx);
    }

    fn dismiss_menu(&mut self, restore_focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.menu.take() else {
            return;
        };
        if restore_focus {
            match &open.restore {
                Some(handle) => window.focus(handle, cx),
                None => window.focus(&self.focus, cx),
            }
        }
        cx.notify();
    }

    /// Routes a key to the open menu; true when a menu took it.
    pub(super) fn menu_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.menu.as_ref() else {
            return false;
        };
        open.view.update(cx, |menu, cx| menu.key(key, cx))
    }

    /// Does what a chosen menu line says.
    pub fn run_menu_action(
        &mut self,
        action: MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            MenuAction::Command(id) => self.run(id, window, cx),
            MenuAction::OpenFile(path) => self.request_open_file(path, window, cx),
            MenuAction::CopyPath(path) => {
                cx.write_to_clipboard(ClipboardItem::new_string(path));
                self.say("Yol panoya kopyalandı", cx);
            }
            MenuAction::TaskStatus { task_id, status } => {
                self.set_task_status(task_id, status, window, cx);
            }
            MenuAction::TaskAssign {
                task_id,
                updated_at,
            } => {
                let selected = self.selected;
                self.assign_task_to_terminal(task_id, updated_at, selected, window, cx);
            }
            MenuAction::TaskClear {
                task_id,
                updated_at,
            } => self.assign_task(task_id, updated_at, None, window, cx),
            MenuAction::TaskHandoff { task_id, title } => {
                self.open_composer(Some(TaskLink { task_id, title }), window, cx);
            }
        }
    }

    // ----- toolbar ---------------------------------------------------------------------

    /// What each toolbar button shows right now, left to right (rules are not listed).
    pub fn toolbar_buttons(&self, cx: &gpui::App) -> Vec<ToolbarButton> {
        let state = self.state(cx);
        toolbar()
            .iter()
            .filter_map(|item| match item {
                ToolbarItem::Separator => None,
                ToolbarItem::Button(id, caption) => {
                    let verdict = available(*id, &state);
                    let command = catalog().iter().find(|command| command.id == *id);
                    let tip = match (command, verdict.reason) {
                        (Some(command), None) => match command.shortcut {
                            Some(shortcut) => format!("{} ({shortcut})", command.label),
                            None => command.label.to_owned(),
                        },
                        (Some(command), Some(reason)) => format!("{} — {reason}", command.label),
                        (None, _) => (*caption).to_owned(),
                    };
                    Some(ToolbarButton {
                        id: *id,
                        caption,
                        enabled: verdict.enabled,
                        pressed: toolbar_pressed(*id, self.dock_visible, self.inspector_visible),
                        tip,
                    })
                }
            })
            .collect()
    }

    /// The row of buttons above the panes.
    pub(super) fn toolbar_row(&self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut buttons = self.toolbar_buttons(cx).into_iter();
        let items = toolbar()
            .iter()
            .enumerate()
            .map(|(index, item)| match item {
                ToolbarItem::Separator => div()
                    .id(SharedString::from(format!("toolbar-rule-{index}")))
                    .flex_none()
                    .mx_1()
                    .h(px(16.0))
                    .w(px(1.0))
                    .bg(gpui::rgb(theme.line))
                    .into_any_element(),
                ToolbarItem::Button(..) => {
                    let Some(shown) = buttons.next() else {
                        return div().into_any_element();
                    };
                    let id = shown.id;
                    let tip = shown.tip.clone();
                    div()
                        .id(SharedString::from(format!("toolbar-{id:?}")))
                        .flex_none()
                        .px_2()
                        .h(px(22.0))
                        .flex()
                        .items_center()
                        .rounded_sm()
                        .text_xs()
                        .whitespace_nowrap()
                        .cursor_pointer()
                        .text_color(gpui::rgb(if shown.enabled {
                            theme.text
                        } else {
                            theme.muted
                        }))
                        .when_pressed(shown.pressed, theme)
                        .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                        .child(shown.caption)
                        .tooltip(move |_window, cx| {
                            let text = SharedString::from(tip.clone());
                            cx.new(|_| Tip(text)).into()
                        })
                        // A disabled button still answers: the command says why it cannot run.
                        .on_click(cx.listener(move |this, _, window, cx| this.run(id, window, cx)))
                        .into_any_element()
                }
            });
        div()
            .id("toolbar")
            .flex_none()
            .h(px(28.0))
            .px_2()
            .flex()
            .flex_row()
            .items_center()
            .gap_0p5()
            .overflow_hidden()
            .bg(gpui::rgb(theme.panel))
            .border_b_1()
            .border_color(gpui::rgb(theme.line))
            .children(items)
            .into_any_element()
    }
}

/// What one toolbar button shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarButton {
    /// The command it runs.
    pub id: CommandId,
    /// Its caption.
    pub caption: &'static str,
    /// The command can run now.
    pub enabled: bool,
    /// The button is a toggle that is on.
    pub pressed: bool,
    /// The tooltip: label and shortcut, or label and the reason it cannot run.
    pub tip: String,
}

/// Styling of a pressed (toggled on) button.
trait WhenPressed: Styled + Sized {
    fn when_pressed(self, pressed: bool, theme: Theme) -> Self {
        if pressed {
            self.bg(gpui::rgb(if theme.lcars {
                theme.accent
            } else {
                theme.selected
            }))
        } else {
            self
        }
    }
}

impl<T: Styled + Sized> WhenPressed for T {}
