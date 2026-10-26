//! Creating, renaming and deleting the files and folders of the open project from the file
//! tree. Every operation goes through the project capability, so none of them can leave the
//! project; a file that is open in the editor is never renamed or deleted underneath it.

use gpui::{AppContext, Context, Window};

use super::Workbench;
use crate::rename::{PromptPurpose, RenamePromptEvent, RenameTerminalPrompt};

/// What the name prompt is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryOp {
    /// A new file inside this folder (empty: the project root).
    NewFile(String),
    /// A new folder inside this folder.
    NewFolder(String),
    /// Rename this entry.
    Rename(String),
}

impl Workbench {
    /// The folder a new entry next to or inside `path` goes to.
    pub(super) fn parent_for_new(path: &str, folder: bool) -> String {
        if folder {
            path.to_owned()
        } else {
            path.rsplit_once('/')
                .map(|(parent, _)| parent.to_owned())
                .unwrap_or_default()
        }
    }

    /// The open editor tabs that are `path` or inside it.
    fn open_tabs_under(&self, path: &str, cx: &gpui::App) -> Vec<String> {
        let prefix = format!("{path}/");
        self.editor
            .read(cx)
            .paths()
            .into_iter()
            .filter(|open| open == path || open.starts_with(&prefix))
            .collect()
    }

    /// Asks for the name for `op`.
    pub fn open_entry_prompt(&mut self, op: EntryOp, window: &mut Window, cx: &mut Context<Self>) {
        if self.rename_prompt.is_some() || self.project.is_none() {
            return;
        }
        if let EntryOp::Rename(path) = &op {
            let open = self.open_tabs_under(path, cx);
            if !open.is_empty() {
                self.say(format!("Önce açık sekmeyi kapatın: {}", open[0]), cx);
                return;
            }
        }
        let theme = self.theme();
        let (purpose, initial) = match &op {
            EntryOp::NewFile(_) => (PromptPurpose::NewFile, String::new()),
            EntryOp::NewFolder(_) => (PromptPurpose::NewFolder, String::new()),
            EntryOp::Rename(path) => (
                PromptPurpose::RenameEntry,
                path.rsplit('/').next().unwrap_or(path).to_owned(),
            ),
        };
        let prompt = cx.new(|cx| RenameTerminalPrompt::entry(purpose, &initial, window, cx));
        prompt.update(cx, |prompt, cx| prompt.set_theme(theme, cx));
        self.rename_subscription = Some(cx.subscribe_in(
            &prompt,
            window,
            move |this, _prompt, event: &RenamePromptEvent, window, cx| match event {
                RenamePromptEvent::EntryName(name) => {
                    this.close_rename_prompt(window, cx);
                    this.apply_entry_op(&op, name, window, cx);
                }
                RenamePromptEvent::Close => this.close_rename_prompt(window, cx),
                RenamePromptEvent::NewTask(_) | RenamePromptEvent::Rename(_) => {}
            },
        ));
        self.rename_prompt = Some(prompt);
        cx.notify();
    }

    fn apply_entry_op(
        &mut self,
        op: &EntryOp,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let result = match op {
            EntryOp::NewFile(parent) => project.create_file(parent, name),
            EntryOp::NewFolder(parent) => project.create_folder(parent, name),
            EntryOp::Rename(path) => {
                let open = self.open_tabs_under(path, cx);
                if !open.is_empty() {
                    self.say(format!("Önce açık sekmeyi kapatın: {}", open[0]), cx);
                    return;
                }
                project.rename_entry(path, name)
            }
        };
        match result {
            Ok(path) => {
                self.files.update(cx, |files, cx| files.refresh(cx));
                match op {
                    EntryOp::NewFile(_) => {
                        self.say(format!("{path} oluşturuldu"), cx);
                        self.request_open_file(path, window, cx);
                    }
                    EntryOp::NewFolder(_) => self.say(format!("{path} klasörü oluşturuldu"), cx),
                    EntryOp::Rename(_) => self.say(format!("Yeni ad: {path}"), cx),
                }
            }
            Err(error) => self.say(error.message, cx),
        }
    }

    /// Asks before deleting `path`.
    pub fn request_delete_entry(&mut self, path: String, folder: bool, cx: &mut Context<Self>) {
        if self.project.is_none() {
            return;
        }
        let open = self.open_tabs_under(&path, cx);
        if !open.is_empty() {
            self.say(format!("Önce açık sekmeyi kapatın: {}", open[0]), cx);
            return;
        }
        self.pending = Some(super::PendingAction::DeleteEntry { path, folder });
        cx.notify();
    }

    pub(super) fn delete_entry(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        match project.delete_entry(path) {
            Ok(()) => {
                self.files.update(cx, |files, cx| files.refresh(cx));
                self.say(format!("{path} silindi"), cx);
            }
            Err(error) => self.say(error.message, cx),
        }
    }
}
