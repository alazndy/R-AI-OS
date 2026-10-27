//! The file tree: a lazily listed view of one project, built on `gpui-base`'s tree component.
//!
//! Directory listings run on the background executor and carry the project generation they
//! were started for; a result for an older generation is dropped, so a slow listing of the
//! previous project can never appear in the new one (spec section 8).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{
    div, AnyElement, AppContext, Context, Entity, EventEmitter, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Window,
};
use gpui_base::{TreeEvent, TreeItem, TreeState};
use rwk_core::project::{FileEntry, ProjectRoot};
use rwk_core::workbench::{presentation, Appearance, Theme};

use crate::assets::icon_path;

/// What the tree tells the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileTreeEvent {
    /// The user chose a file (relative path). The workbench decides whether it may open.
    OpenFile(String),
    /// The user right-clicked a row (relative path); the row is selected.
    ContextMenu {
        /// Relative path of the row.
        path: String,
        /// The row is a folder.
        folder: bool,
        /// Where the pointer was.
        position: gpui::Point<gpui::Pixels>,
    },
}

/// Marks the placeholder child of a folder that has not been listed yet.
const LOADING_SUFFIX: &str = "/\u{1}loading";

/// The tree of one project.
pub struct FileTreeView {
    tree: Entity<TreeState>,
    project: Option<Arc<ProjectRoot>>,
    /// Bumped by `set_project`; listings of an older generation are dropped.
    generation: u64,
    /// Listed folders (the root is the empty path) and their entries.
    nodes: HashMap<String, Vec<FileEntry>>,
    /// Folders the user has expanded.
    expanded: HashSet<String>,
    /// Folders whose listing is in flight.
    pending: HashSet<String>,
    status: Option<SharedString>,
    /// Git marks by project-relative path (files and the folders above changed files).
    badges: HashMap<String, rwk_core::git::Badge>,
    focus: FocusHandle,
    theme: Theme,
    _subscription: Subscription,
}

impl EventEmitter<FileTreeEvent> for FileTreeView {}

impl FileTreeView {
    /// An empty tree.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| TreeState::new(cx));
        let subscription = cx.subscribe(&tree, |this, _tree, event: &TreeEvent, cx| match event {
            TreeEvent::Expanded(id) => this.expand(id.as_ref(), cx),
            TreeEvent::Collapsed(id) => {
                this.expanded.remove(id.as_ref());
            }
        });
        Self {
            tree,
            project: None,
            generation: 0,
            nodes: HashMap::new(),
            expanded: HashSet::new(),
            pending: HashSet::new(),
            status: None,
            badges: HashMap::new(),
            focus: cx.focus_handle(),
            theme: Theme::of(Appearance::default()),
            _subscription: subscription,
        }
    }

    /// Shows `project`; whatever was listed for the previous one is discarded.
    pub fn set_project(&mut self, project: Arc<ProjectRoot>, cx: &mut Context<Self>) {
        self.generation += 1;
        self.project = Some(project);
        self.nodes.clear();
        self.expanded.clear();
        self.pending.clear();
        self.status = None;
        self.rebuild(cx);
        self.list(String::new(), cx);
    }

    /// Replaces the git marks shown next to files and folders.
    pub fn set_badges(
        &mut self,
        badges: HashMap<String, rwk_core::git::Badge>,
        cx: &mut Context<Self>,
    ) {
        if self.badges != badges {
            self.badges = badges;
            cx.notify();
        }
    }

    /// The git mark of `path`, when it has one.
    pub fn badge(&self, path: &str) -> Option<rwk_core::git::Badge> {
        self.badges.get(path).copied()
    }

    /// Lists the root and every expanded folder again, keeping expansion and selection.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.project.is_none() {
            return;
        }
        let mut folders: Vec<String> = self.expanded.iter().cloned().collect();
        folders.push(String::new());
        for folder in folders {
            self.list(folder, cx);
        }
    }

    /// Expands a folder, listing it when it has not been listed yet.
    pub fn expand(&mut self, path: &str, cx: &mut Context<Self>) {
        self.expanded.insert(path.to_owned());
        if self.nodes.contains_key(path) {
            self.rebuild(cx);
        } else {
            self.list(path.to_owned(), cx);
            self.rebuild(cx);
        }
    }

    fn list(&mut self, folder: String, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        if !self.pending.insert(folder.clone()) {
            return;
        }
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let target = folder.clone();
            let result = cx
                .background_executor()
                .spawn(async move { project.list(&target) })
                .await;
            this.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                view.pending.remove(&folder);
                match result {
                    Ok(entries) => {
                        view.nodes.insert(folder, entries);
                        view.status = None;
                    }
                    Err(error) => view.status = Some(SharedString::from(error.message)),
                }
                view.rebuild(cx);
            })
            .ok();
        })
        .detach();
    }

    fn items_for(&self, folder: &str) -> Vec<TreeItem> {
        let Some(entries) = self.nodes.get(folder) else {
            return Vec::new();
        };
        entries
            .iter()
            .map(|entry| {
                let item = TreeItem::new(entry.path.clone(), entry.name.clone());
                if !entry.is_directory {
                    return item;
                }
                let children = if self.nodes.contains_key(&entry.path) {
                    self.items_for(&entry.path)
                } else {
                    // Unlisted folders get a disabled placeholder so they can be expanded.
                    vec![
                        TreeItem::new(format!("{}{LOADING_SUFFIX}", entry.path), "…")
                            .disabled(true),
                    ]
                };
                item.children(children)
                    .expanded(self.expanded.contains(&entry.path))
            })
            .collect()
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let items = self.items_for("");
        let selected = self.selected_path(cx);
        self.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            if let Some(path) = selected {
                if let Some(ix) = tree.index_of(&SharedString::from(path)) {
                    tree.set_selected_index(Some(ix), cx);
                }
            }
        });
        cx.notify();
    }

    /// The ids (relative paths) of the visible rows, top to bottom.
    pub fn visible_ids(&self, cx: &gpui::App) -> Vec<String> {
        let tree = self.tree.read(cx);
        (0..)
            .map_while(|ix| tree.entry(ix))
            .map(|entry| entry.item().id.to_string())
            .filter(|id| !id.ends_with(LOADING_SUFFIX))
            .collect()
    }

    /// The selected row's path.
    pub fn selected_path(&self, cx: &gpui::App) -> Option<String> {
        self.tree
            .read(cx)
            .selected_item()
            .map(|item| item.id.to_string())
    }

    /// Expands a collapsed folder or collapses an expanded one.
    pub fn toggle(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.expanded.contains(path) {
            self.expanded.remove(path);
            self.rebuild(cx);
        } else {
            self.expand(path, cx);
        }
    }

    /// Selects a row by path (no event).
    pub fn select(&mut self, path: &str, cx: &mut Context<Self>) {
        let id = SharedString::from(path.to_owned());
        self.tree.update(cx, |tree, cx| {
            if let Some(ix) = tree.index_of(&id) {
                tree.set_selected_index(Some(ix), cx);
            }
        });
    }

    fn is_file(&self, path: &str) -> bool {
        let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        self.nodes
            .get(parent)
            .and_then(|entries| entries.iter().find(|entry| entry.path == path))
            .is_some_and(|entry| !entry.is_directory)
    }

    /// The user chose `path` (a click or Enter). Files emit [`FileTreeEvent::OpenFile`]; folders
    /// and unknown paths do nothing.
    pub fn activate(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.is_file(path) {
            cx.emit(FileTreeEvent::OpenFile(path.to_owned()));
        }
    }

    /// Repaints the tree with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Gives the tree keyboard focus.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }

    /// The message shown under the tree (a failed listing).
    pub fn status(&self) -> Option<String> {
        self.status.as_ref().map(ToString::to_string)
    }

    /// Moves the selection `step` rows (clamped); selects the first row when nothing is selected.
    fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        let ids = self.visible_ids(cx);
        if ids.is_empty() {
            return;
        }
        let current = self
            .selected_path(cx)
            .and_then(|selected| ids.iter().position(|id| *id == selected));
        let next = match current {
            Some(ix) => ix.saturating_add_signed(step).min(ids.len() - 1),
            None => 0,
        };
        self.select(&ids[next], cx);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "down" => self.move_selection(1, cx),
            "up" => self.move_selection(-1, cx),
            "right" => {
                if let Some(path) = self.selected_path(cx) {
                    if !self.is_file(&path) && !self.expanded.contains(&path) {
                        self.expand(&path, cx);
                    }
                }
            }
            "left" => {
                if let Some(path) = self.selected_path(cx) {
                    if self.expanded.remove(&path) {
                        self.rebuild(cx);
                    }
                }
            }
            "enter" => {
                if let Some(path) = self.selected_path(cx) {
                    if self.is_file(&path) {
                        self.activate(&path, cx);
                    } else {
                        self.toggle(&path, cx);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The colour of a git mark: bright on a dark tone, deep on a light one.
pub(crate) fn badge_color(kind: rwk_core::git::BadgeKind, dark: bool) -> u32 {
    use rwk_core::git::BadgeKind;
    match (kind, dark) {
        (BadgeKind::Added, true) => 0x3fb950,
        (BadgeKind::Added, false) => 0x1a7f37,
        (BadgeKind::Modified, true) => 0xffa657,
        (BadgeKind::Modified, false) => 0xb35900,
        (BadgeKind::Deleted | BadgeKind::Conflict, true) => 0xff7b72,
        (BadgeKind::Deleted | BadgeKind::Conflict, false) => 0xc62828,
    }
}

impl Render for FileTreeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The rows are drawn here rather than by `gpui_base::Tree`'s virtual list, which painted
        // nothing in this setup; the component's state still owns the flattened entries.
        let selected = self.selected_path(cx);
        let rows: Vec<_> = {
            let tree = self.tree.read(cx);
            (0..)
                .map_while(|ix| tree.entry(ix))
                .filter(|entry| !entry.item().id.ends_with(LOADING_SUFFIX))
                .map(|entry| {
                    (
                        entry.item().id.to_string(),
                        entry.item().label.to_string(),
                        entry.depth(),
                        entry.is_folder(),
                        entry.is_expanded(),
                    )
                })
                .collect()
        };
        let theme = self.theme;
        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(ix, (id, label, depth, folder, open))| {
                let look = presentation(&label, folder, open);
                let badge = self.badges.get(&id).copied();
                let menu_id = id.clone();
                let is_selected = selected.as_deref() == Some(id.as_str());
                let chevron = match (folder, open) {
                    (true, true) => "▾",
                    (true, false) => "▸",
                    (false, _) => " ",
                };
                // On a filled selection the glyph takes the fill's ink colour, as in 0.1.
                let on_fill = theme.lcars && is_selected;
                let glyph_color = if on_fill {
                    theme.on_fill
                } else {
                    look.tone.color(theme.tone())
                };
                let mut row = div()
                    .id(ix)
                    .w_full()
                    .h(gpui::px(ROW_HEIGHT))
                    .min_w_0()
                    .overflow_hidden()
                    .pl(gpui::px(6.0 + INDENT * depth as f32))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .text_color(gpui::rgb(if on_fill { theme.on_fill } else { theme.text }))
                    .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                    .child(
                        div()
                            .flex_none()
                            .w(gpui::px(10.0))
                            .text_xs()
                            .text_color(gpui::rgb(if on_fill {
                                theme.on_fill
                            } else {
                                theme.blue_ink
                            }))
                            .child(chevron),
                    )
                    .child(
                        gpui::svg()
                            .path(icon_path(look.icon))
                            .size(gpui::px(16.0))
                            .flex_none()
                            .text_color(gpui::rgb(glyph_color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(label),
                    )
                    .children(badge.map(|badge| {
                        div()
                            .flex_none()
                            .pr_2()
                            .text_xs()
                            .text_color(gpui::rgb(badge_color(badge.kind, theme.dark)))
                            .child(SharedString::from(badge.letter.to_string()))
                    }))
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |view, event: &gpui::MouseDownEvent, _window, cx| {
                            view.select(&menu_id, cx);
                            cx.emit(FileTreeEvent::ContextMenu {
                                path: menu_id.clone(),
                                folder,
                                position: event.position,
                            });
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(move |view, _, _window, cx| {
                        view.select(&id, cx);
                        if folder {
                            view.toggle(&id, cx);
                        } else {
                            view.activate(&id, cx);
                        }
                    }));
                if theme.lcars {
                    row = row.rounded_r(gpui::px(14.0));
                }
                if is_selected {
                    row = if theme.lcars {
                        row.bg(gpui::rgb(theme.accent))
                    } else {
                        row.bg(gpui::rgb(theme.selected))
                            .border_l_2()
                            .border_color(gpui::rgb(theme.accent))
                    };
                }
                row
            });
        let title = self
            .project
            .as_ref()
            .map(|project| project.info().name.to_uppercase())
            .unwrap_or_default();
        let status = self.status.clone().unwrap_or_default();
        let element: AnyElement = div()
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(if theme.lcars {
                // The LCARS pane title: a blue bar with display lettering.
                div()
                    .h(gpui::px(40.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .bg(gpui::rgb(theme.blue))
                    .rounded_tl(gpui::px(16.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(gpui::rgb(theme.on_fill))
                    .font_family(crate::fonts::display_family(window))
                    .text_size(gpui::px(16.0))
                    .child(title)
            } else {
                div()
                    .h(gpui::px(28.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(gpui::rgb(theme.line))
                    .text_xs()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(gpui::rgb(theme.muted))
                    .child(title)
            })
            .child(
                div()
                    .id("file-tree-rows")
                    .flex_1()
                    .min_h_0()
                    .py_1()
                    .overflow_y_scroll()
                    // The empty part below the rows (rows stop their own right clicks): the
                    // project root, so a file can be created at the top level.
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(|_, event: &gpui::MouseDownEvent, _window, cx| {
                            cx.emit(FileTreeEvent::ContextMenu {
                                path: String::new(),
                                folder: true,
                                position: event.position,
                            });
                            cx.stop_propagation();
                        }),
                    )
                    .children(rows),
            )
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(gpui::rgb(theme.bad))
                    .child(status),
            )
            .into_any_element();
        element
    }
}

const ROW_HEIGHT: f32 = 24.0;
const INDENT: f32 = 14.0;
