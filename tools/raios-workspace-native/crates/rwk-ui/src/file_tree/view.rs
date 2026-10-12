//! The file tree: a lazily listed view of one project, built on `gpui-base`'s tree component.
//!
//! Directory listings run on the background executor and carry the project generation they
//! were started for; a result for an older generation is dropped, so a slow listing of the
//! previous project can never appear in the new one (spec section 8).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{
    div, AnyElement, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window,
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
        self.tree.update(cx, |tree, cx| tree.focus(window, cx));
    }

    /// The message shown under the tree (a failed listing).
    pub fn status(&self) -> Option<String> {
        self.status.as_ref().map(ToString::to_string)
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "enter" {
            if let Some(path) = self.selected_path(cx) {
                self.activate(&path, cx);
            }
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
                let marker = match (folder, open) {
                    (true, true) => "▾ ",
                    (true, false) => "▸ ",
                    (false, _) => "  ",
                };
                let text = if folder {
                    format!("{marker}{label}/")
                } else {
                    format!("{marker}{label}")
                };
                let is_selected = selected.as_deref() == Some(id.as_str());
                let mut row = div()
                    .id(ix)
                    .w_full()
                    .h(gpui::px(22.0))
                    .pl(gpui::px(8.0 + 14.0 * depth as f32))
                    .text_color(gpui::rgb(0xd4d4d4))
                    .child(text)
                    .on_click(cx.listener(move |view, _, _window, cx| {
                        view.select(&id, cx);
                        if folder {
                            view.toggle(&id, cx);
                        } else {
                            view.activate(&id, cx);
                        }
                    }));
                if is_selected {
                    row = row.bg(gpui::rgb(0x37373d));
                }
                row
            });
        let status = self.status.clone().unwrap_or_default();
        let element: AnyElement = div()
            .size_full()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(
                div()
                    .id("file-tree-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(div().px_2().child(status))
            .into_any_element();
        element
    }
}
