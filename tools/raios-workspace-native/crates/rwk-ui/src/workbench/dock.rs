//! Draws the terminal dock from its [`DockLayout`] tree and turns gestures into changes of it.
//!
//! Splits become rows and columns sized by their weights, a group of one pane is that pane under
//! its title strip, a group of several is a strip of tabs over the active pane. A title can be
//! dragged: onto another title it becomes a tab there, onto the middle of a pane it joins that
//! pane's group, onto the edge band of a pane it is split off on that edge. The dividers between
//! the children of a split can be dragged too. Every gesture has a palette command that does the
//! same without the mouse. The arrangement and its rules live in the core; this file only paints
//! and forwards.

use gpui::{
    div, px, relative, AnyElement, AppContext, ClickEvent, Context, DragMoveEvent,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window,
};
use rwk_core::sessions::SessionId;
use rwk_core::workbench::{zone_at, Axis, CommandId, DockLayout, DropZone, Edge, Node, Theme};

use super::{SelectedPane, Workbench};
use crate::inspector::DraggedTask;

/// What a dragged terminal title carries.
#[derive(Clone)]
pub struct DraggedTerminal {
    /// The terminal being moved.
    pub id: SessionId,
    title: SharedString,
}

/// What a dragged divider carries: which split, and which of its boundaries.
#[derive(Clone)]
pub struct DraggedDivider {
    path: Vec<usize>,
    boundary: usize,
}

/// The small label that follows the pointer while a title is dragged.
struct DragPreview {
    label: SharedString,
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_0p5()
            .text_xs()
            .rounded_sm()
            .bg(gpui::rgba(0x000000cc))
            .text_color(gpui::rgb(0xffffff))
            .child(self.label.clone())
    }
}

/// Width of the grab area of a divider, in pixels.
const DIVIDER_PX: f32 = 6.0;

/// How much one grow or shrink command moves a boundary, as a fraction of the split.
const RESIZE_STEP: f32 = 0.05;

impl Workbench {
    /// How the dock arranges the terminals right now.
    pub fn dock(&self) -> &DockLayout<SessionId> {
        &self.dock
    }

    /// The session of terminal `index`, in creation order.
    pub fn terminal_id(&self, index: usize) -> Option<SessionId> {
        self.terminals.get(index).map(|pane| pane.id.clone())
    }

    /// Where a dragged terminal would land if it were dropped now, for the highlight.
    pub fn drop_hint(&self) -> Option<&(SessionId, DropZone)> {
        self.drop_hint.as_ref()
    }

    // ----- arranging -------------------------------------------------------------------

    /// Applies one arrangement command to the selected terminal. A refusal is a toast with the
    /// reason; the layout is unchanged then.
    pub(super) fn arrange(
        &mut self,
        command: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self
            .terminals
            .get(self.selected)
            .map(|pane| pane.id.clone())
        else {
            return;
        };
        let before = self.dock.neighbor(&id, -1);
        let after = self.dock.neighbor(&id, 1);
        let result = match command {
            CommandId::MoveTerminalPrevious => before
                .ok_or(rwk_core::workbench::DockError("it is already first"))
                .and_then(|other| self.dock.swap(&id, &other)),
            CommandId::MoveTerminalNext => after
                .ok_or(rwk_core::workbench::DockError("it is already last"))
                .and_then(|other| self.dock.swap(&id, &other)),
            CommandId::SplitTerminalRight => before
                .or(after)
                .ok_or(rwk_core::workbench::DockError("there is no neighbour"))
                .and_then(|other| self.dock.split(&id, &other, Edge::Right)),
            CommandId::SplitTerminalDown => before
                .or(after)
                .ok_or(rwk_core::workbench::DockError("there is no neighbour"))
                .and_then(|other| self.dock.split(&id, &other, Edge::Bottom)),
            CommandId::JoinTerminalPrevious => before
                .ok_or(rwk_core::workbench::DockError("it is already first"))
                .and_then(|other| self.dock.move_tab(&id, &other, usize::MAX)),
            CommandId::UngroupTerminal => self.dock.ungroup(&id),
            CommandId::GrowTerminal => self.dock.grow(&id, RESIZE_STEP),
            CommandId::ShrinkTerminal => self.dock.grow(&id, -RESIZE_STEP),
            _ => return,
        };
        self.after_arrange(&id, result, window, cx);
    }

    /// A dragged terminal was dropped on `target` in `zone`.
    pub fn drop_terminal(
        &mut self,
        dragged: &SessionId,
        target: &SessionId,
        zone: DropZone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drop_hint = None;
        let result = match zone {
            _ if dragged == target && zone == DropZone::Center => Ok(()),
            DropZone::Center => self.dock.move_tab(dragged, target, usize::MAX),
            DropZone::Edge(edge) => self.dock.split(dragged, target, edge),
        };
        self.after_arrange(dragged, result, window, cx);
    }

    /// A dragged terminal was dropped on the title of another: it takes that tab's place.
    pub fn drop_on_title(
        &mut self,
        dragged: &SessionId,
        target: &SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drop_hint = None;
        let result = if dragged == target {
            Ok(())
        } else {
            let at = self.dock.tab_position(target).unwrap_or(usize::MAX);
            self.dock.move_tab(dragged, target, at)
        };
        self.after_arrange(dragged, result, window, cx);
    }

    /// A divider was dragged to `position`, a fraction of its split.
    pub fn drag_divider(
        &mut self,
        path: &[usize],
        boundary: usize,
        position: f32,
        cx: &mut Context<Self>,
    ) {
        if self.dock.set_boundary(path, boundary, position).is_ok() {
            self.note_workspace_change(cx);
            cx.notify();
        }
    }

    fn after_arrange(
        &mut self,
        moved: &SessionId,
        result: Result<(), rwk_core::workbench::DockError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => {
                if let Some(index) = self.terminals.iter().position(|pane| &pane.id == moved) {
                    self.select_terminal(index, window, cx);
                }
                self.note_workspace_change(cx);
            }
            Err(error) => self.say(format!("Terminal taşınamadı: {error}"), cx),
        }
        cx.notify();
    }

    /// A task row was dropped on terminal `target`: the task goes to that terminal's agent.
    pub fn drop_task(
        &mut self,
        dragged: &DraggedTask,
        target: &SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.terminals.iter().position(|pane| &pane.id == target) else {
            return;
        };
        self.assign_task_to_terminal(
            dragged.task_id.clone(),
            dragged.updated_at.clone(),
            index,
            window,
            cx,
        );
    }

    // ----- painting --------------------------------------------------------------------

    /// The whole terminal area, or `None` while there is no terminal.
    pub(super) fn dock_tree(&self, theme: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let root = self.dock.root()?;
        Some(
            div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(self.dock_node(root, &[], theme, cx))
                .into_any_element(),
        )
    }

    fn dock_node(
        &self,
        node: &Node<SessionId>,
        path: &[usize],
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            Node::Tabs { sessions, active } => {
                let shown = sessions.get(*active).or_else(|| sessions.first());
                let Some(shown) = shown else {
                    return div().into_any_element();
                };
                let strip =
                    (sessions.len() > 1).then(|| self.tab_strip(sessions, shown, theme, cx));
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .children(strip)
                    .child(self.pane(shown, sessions.len() == 1, theme, cx))
                    .into_any_element()
            }
            Node::Split { axis, children } => {
                let row = *axis == Axis::Horizontal;
                let mut offset = 0.0_f32;
                let mut dividers: Vec<AnyElement> = Vec::new();
                let mut cells: Vec<AnyElement> = Vec::new();
                for (index, child) in children.iter().enumerate() {
                    let mut child_path = path.to_vec();
                    child_path.push(index);
                    let cell = div().flex_none().min_h_0().min_w_0().overflow_hidden();
                    let cell = if row {
                        cell.h_full().w(relative(child.weight))
                    } else {
                        cell.w_full().h(relative(child.weight))
                    };
                    cells.push(
                        cell.child(self.dock_node(&child.node, &child_path, theme, cx))
                            .into_any_element(),
                    );
                    offset += child.weight;
                    if index + 1 < children.len() {
                        dividers.push(self.divider(path, index, row, offset, theme));
                    }
                }
                let owner = path.to_vec();
                let split = div()
                    .id(SharedString::from(format!("dock-split-{path:?}")))
                    .size_full()
                    .relative()
                    .flex()
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<DraggedDivider>, _window, cx| {
                            let dragged = event.drag(cx);
                            if dragged.path != owner {
                                return;
                            }
                            let (start, length, pointer) = if row {
                                (
                                    event.bounds.origin.x,
                                    event.bounds.size.width,
                                    event.event.position.x,
                                )
                            } else {
                                (
                                    event.bounds.origin.y,
                                    event.bounds.size.height,
                                    event.event.position.y,
                                )
                            };
                            if length > px(0.0) {
                                let position = (pointer - start) / length;
                                let (path, boundary) = (dragged.path.clone(), dragged.boundary);
                                this.drag_divider(&path, boundary, position, cx);
                            }
                        },
                    ));
                let split = if row {
                    split.flex_row()
                } else {
                    split.flex_col()
                };
                split.children(cells).children(dividers).into_any_element()
            }
        }
    }

    /// The grab area between child `index` and the next, `offset` into the split.
    fn divider(
        &self,
        path: &[usize],
        index: usize,
        row: bool,
        offset: f32,
        theme: Theme,
    ) -> AnyElement {
        let carried = DraggedDivider {
            path: path.to_vec(),
            boundary: index,
        };
        let handle = div()
            .id(SharedString::from(format!("dock-divider-{path:?}-{index}")))
            .absolute()
            .hover(move |style| style.bg(gpui::rgb(theme.accent_ink)))
            .on_drag(carried, |_dragged, _offset, _window, cx| {
                cx.new(|_| DragPreview {
                    label: SharedString::default(),
                })
            });
        if row {
            handle
                .top_0()
                .h_full()
                .w(px(DIVIDER_PX))
                .ml(px(-DIVIDER_PX / 2.0))
                .left(relative(offset))
                .cursor_col_resize()
                .into_any_element()
        } else {
            handle
                .left_0()
                .w_full()
                .h(px(DIVIDER_PX))
                .mt(px(-DIVIDER_PX / 2.0))
                .top(relative(offset))
                .cursor_row_resize()
                .into_any_element()
        }
    }

    /// One terminal pane: its view, and above it the title strip when the pane is alone in its
    /// group (a group of several shows the titles in its tab strip instead). The body is a drop
    /// target with an edge band on every side.
    fn pane(
        &self,
        id: &SessionId,
        with_title: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(index) = self.terminals.iter().position(|pane| &pane.id == id) else {
            return div().into_any_element();
        };
        let pane = &self.terminals[index];
        let title = with_title.then(|| {
            self.title_cell(id, index, theme, cx)
                .border_b_1()
                .border_color(gpui::rgb(theme.line))
                .bg(gpui::rgb(theme.panel))
        });
        let hint = self
            .drop_hint
            .as_ref()
            .filter(|(target, _)| target == id)
            .map(|(_, zone)| *zone);
        let on_move = id.clone();
        let on_drop = id.clone();
        let on_task_drop = id.clone();
        let on_menu = id.clone();
        let body = div()
            .id(SharedString::from(format!("terminal-body-{}", id.as_str())))
            .flex_1()
            .min_h_0()
            .relative()
            .child(pane.view.clone())
            .children(hint.map(|zone| drop_highlight(zone, theme)))
            // Capture phase: the terminal view would otherwise take the click first. A program
            // that asked for mouse reports keeps its right click; Shift takes it back.
            .capture_any_mouse_down(cx.listener(
                move |this, event: &gpui::MouseDownEvent, window, cx| {
                    if event.button != gpui::MouseButton::Right {
                        return;
                    }
                    let Some(index) = this.terminals.iter().position(|pane| pane.id == on_menu)
                    else {
                        return;
                    };
                    let wants = this.terminals[index].view.read(cx).wants_mouse();
                    if wants && !event.modifiers.shift {
                        return;
                    }
                    this.select_terminal(index, window, cx);
                    this.open_context_menu(
                        rwk_core::workbench::MenuTarget::TerminalBody,
                        event.position,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                },
            ))
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<DraggedTerminal>, _window, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        if this
                            .drop_hint
                            .as_ref()
                            .is_some_and(|(target, _)| *target == on_move)
                        {
                            this.drop_hint = None;
                            cx.notify();
                        }
                        return;
                    }
                    let size = event.bounds.size;
                    let local = event.event.position - event.bounds.origin;
                    let zone = zone_at(local.x / size.width, local.y / size.height);
                    let next = Some((on_move.clone(), zone));
                    if this.drop_hint != next {
                        this.drop_hint = next;
                        cx.notify();
                    }
                },
            ))
            .on_drop(cx.listener(move |this, dragged: &DraggedTask, window, cx| {
                this.drop_task(dragged, &on_task_drop, window, cx);
            }))
            .on_drop(
                cx.listener(move |this, dragged: &DraggedTerminal, window, cx| {
                    let zone = this
                        .drop_hint
                        .as_ref()
                        .filter(|(target, _)| *target == on_drop)
                        .map_or(DropZone::Center, |(_, zone)| *zone);
                    this.drop_terminal(&dragged.id, &on_drop, zone, window, cx);
                }),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .border_1()
            .when_selected(theme, index == self.selected)
            .children(title)
            .child(body)
            .into_any_element()
    }

    fn tab_strip(
        &self,
        sessions: &[SessionId],
        shown: &SessionId,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cells: Vec<AnyElement> = sessions
            .iter()
            .filter_map(|id| {
                let index = self.terminals.iter().position(|pane| &pane.id == id)?;
                let cell = self
                    .title_cell(id, index, theme, cx)
                    .px_3()
                    .border_r_1()
                    .border_color(gpui::rgb(theme.line))
                    .bg(gpui::rgb(if id == shown {
                        theme.raised
                    } else {
                        theme.panel
                    }));
                Some(cell.into_any_element())
            })
            .collect();
        div()
            .flex_none()
            .flex()
            .flex_row()
            .border_b_1()
            .border_color(gpui::rgb(theme.line))
            .children(cells)
            .into_any_element()
    }

    /// The title of one terminal: a click selects it, a double click renames it, dragging moves
    /// it, and dropping another title on it puts that terminal in its place.
    fn title_cell(
        &self,
        id: &SessionId,
        index: usize,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let pane = &self.terminals[index];
        let title = SharedString::from(self.terminal_title(index));
        let click_target = id.clone();
        let drop_target = id.clone();
        let task_target = id.clone();
        let menu_target = id.clone();
        let close_target = id.clone();
        let middle_target = id.clone();
        let carried = DraggedTerminal {
            id: id.clone(),
            title: title.clone(),
        };
        div()
            .id(SharedString::from(format!(
                "terminal-title-{}",
                id.as_str()
            )))
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .min_h(px(18.0))
            .text_xs()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_color(gpui::rgb(if pane.name.is_some() {
                theme.text
            } else {
                theme.muted
            }))
            .child(title)
            .child(
                div()
                    .id(SharedString::from(format!(
                        "terminal-close-{}",
                        id.as_str()
                    )))
                    .flex_none()
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(gpui::rgb(theme.muted))
                    .hover(move |style| {
                        style
                            .bg(gpui::rgb(theme.raised))
                            .text_color(gpui::rgb(theme.bad))
                    })
                    .child("×")
                    // A press on the cross must not start dragging the title.
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(
                        move |this: &mut Workbench, _: &ClickEvent, window: &mut Window, cx| {
                            cx.stop_propagation();
                            this.close_terminal_by_id(&close_target, window, cx);
                        },
                    )),
            )
            .on_mouse_down(
                gpui::MouseButton::Middle,
                cx.listener(
                    move |this: &mut Workbench, _: &gpui::MouseDownEvent, window, cx| {
                        this.close_terminal_by_id(&middle_target, window, cx);
                        cx.stop_propagation();
                    },
                ),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(
                    move |this: &mut Workbench, event: &gpui::MouseDownEvent, window, cx| {
                        let Some(index) = this
                            .terminals
                            .iter()
                            .position(|pane| pane.id == menu_target)
                        else {
                            return;
                        };
                        this.select_terminal(index, window, cx);
                        this.open_context_menu(
                            rwk_core::workbench::MenuTarget::TerminalTitle,
                            event.position,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    },
                ),
            )
            .on_click(cx.listener(
                move |this: &mut Workbench, event: &ClickEvent, window: &mut Window, cx| {
                    // The index is looked up again: terminals may have come and gone since the paint.
                    let Some(index) = this
                        .terminals
                        .iter()
                        .position(|pane| pane.id == click_target)
                    else {
                        return;
                    };
                    if event.click_count() >= 2 {
                        this.open_rename_prompt(index, window, cx);
                    } else {
                        this.select_terminal(index, window, cx);
                    }
                },
            ))
            .on_drag(carried, |dragged, _offset, _window, cx| {
                let label = dragged.title.clone();
                cx.new(|_| DragPreview { label })
            })
            .drag_over::<DraggedTerminal>(move |style, _dragged, _window, _cx| {
                style.bg(gpui::rgb(theme.raised))
            })
            .drag_over::<DraggedTask>(move |style, _dragged, _window, _cx| {
                style.bg(gpui::rgb(theme.raised))
            })
            .on_drop(cx.listener(move |this, dragged: &DraggedTask, window, cx| {
                this.drop_task(dragged, &task_target, window, cx);
            }))
            .on_drop(
                cx.listener(move |this, dragged: &DraggedTerminal, window, cx| {
                    this.drop_on_title(&dragged.id, &drop_target, window, cx);
                }),
            )
    }
}

/// The translucent band that shows where a drop would land: a half of the pane for an edge, the
/// whole pane for the middle.
fn drop_highlight(zone: DropZone, theme: Theme) -> AnyElement {
    let band = div()
        .absolute()
        .bg(gpui::rgba((theme.accent_ink << 8) | 0x55))
        .border_1()
        .border_color(gpui::rgb(theme.accent_ink));
    match zone {
        DropZone::Center => band.size_full().into_any_element(),
        DropZone::Edge(Edge::Left) => band
            .top_0()
            .left_0()
            .h_full()
            .w(relative(0.5))
            .into_any_element(),
        DropZone::Edge(Edge::Right) => band
            .top_0()
            .right_0()
            .h_full()
            .w(relative(0.5))
            .into_any_element(),
        DropZone::Edge(Edge::Top) => band
            .top_0()
            .left_0()
            .w_full()
            .h(relative(0.5))
            .into_any_element(),
        DropZone::Edge(Edge::Bottom) => band
            .bottom_0()
            .left_0()
            .w_full()
            .h(relative(0.5))
            .into_any_element(),
    }
}
