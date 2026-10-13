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

/// What the tree tells the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileTreeEvent {
    /// The user chose a file (relative path). The workbench decides whether it may open.
    OpenFile(String),
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
    focus: FocusHandle,
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
            focus: cx.focus_handle(),
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

impl Render for FileTreeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(ix, (id, label, depth, folder, open))| {
                let icon = icon_for(&label, folder, open);
                let is_selected = selected.as_deref() == Some(id.as_str());
                let mut row = div()
                    .id(ix)
                    .w_full()
                    .h(gpui::px(ROW_HEIGHT))
                    .pl(gpui::px(8.0 + INDENT * depth as f32))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .text_color(gpui::rgb(if folder { 0xe8e8e8 } else { 0xcfcfcf }))
                    .hover(|style| style.bg(gpui::rgb(HOVER_BACKGROUND)))
                    .child(
                        div()
                            .w(gpui::px(14.0))
                            .flex()
                            .justify_center()
                            .text_color(gpui::rgb(icon.color))
                            .child(icon.glyph),
                    )
                    .child(label)
                    .on_click(cx.listener(move |view, _, _window, cx| {
                        view.select(&id, cx);
                        if folder {
                            view.toggle(&id, cx);
                        } else {
                            view.activate(&id, cx);
                        }
                    }));
                if is_selected {
                    row = row
                        .bg(gpui::rgb(SELECTED_BACKGROUND))
                        .border_l_2()
                        .border_color(gpui::rgb(ACCENT));
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
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(
                div()
                    .h(gpui::px(28.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(gpui::rgb(0x333333))
                    .text_xs()
                    .text_color(gpui::rgb(0x9a9a9a))
                    .child(title),
            )
            .child(
                div()
                    .id("file-tree-rows")
                    .flex_1()
                    .min_h_0()
                    .py_1()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(gpui::rgb(0xf14c4c))
                    .child(status),
            )
            .into_any_element();
        element
    }
}

const ROW_HEIGHT: f32 = 24.0;
const INDENT: f32 = 14.0;
const ACCENT: u32 = 0x3794ff;
const HOVER_BACKGROUND: u32 = 0x2a2d2e;
const SELECTED_BACKGROUND: u32 = 0x37373d;

/// The glyph and colour drawn in front of a tree row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowIcon {
    /// What is drawn.
    pub glyph: &'static str,
    /// 0xRRGGBB.
    pub color: u32,
}

/// The icon of a row: an open/closed chevron for folders, a coloured mark by file type otherwise.
pub fn icon_for(name: &str, is_folder: bool, is_open: bool) -> RowIcon {
    if is_folder {
        let glyph = if is_open { "▾" } else { "▸" };
        return RowIcon {
            glyph,
            color: 0xdcb67a,
        };
    }
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    let color = match extension {
        "rs" => 0xe8845c,
        "toml" | "lock" => 0xb180d7,
        "md" | "txt" => 0x6fb3ff,
        "json" | "yaml" | "yml" => 0xe5c07b,
        "ts" | "tsx" | "js" | "jsx" => 0x4ec9f0,
        "py" => 0x6bc46d,
        "sh" | "bash" | "zsh" => 0x89d185,
        "kt" | "java" => 0xf0883e,
        "css" | "html" => 0xe06c9f,
        _ => 0x8a8a8a,
    };
    RowIcon {
        glyph: "●", color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_get_a_chevron_that_follows_their_state() {
        assert_eq!(icon_for("src", true, false).glyph, "▸");
        assert_eq!(icon_for("src", true, true).glyph, "▾");
    }

    #[test]
    fn files_are_coloured_by_type_and_unknown_types_are_grey() {
        assert_ne!(
            icon_for("main.rs", false, false).color,
            icon_for("Cargo.toml", false, false).color
        );
        assert_eq!(icon_for("LICENSE", false, false).color, 0x8a8a8a);
        assert_eq!(icon_for(".gitignore", false, false).color, 0x8a8a8a);
    }
}
