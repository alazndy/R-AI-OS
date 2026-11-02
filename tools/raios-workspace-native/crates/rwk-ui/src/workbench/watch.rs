//! Noticing changes made outside the workbench: another tool saved an open file, or files
//! appeared in a folder the tree shows. A cheap stamp of each open file and folder is checked
//! every few seconds; only a changed one is read again.

use std::collections::HashMap;
use std::time::Duration;

use gpui::{Context, Window};

use super::Workbench;

/// How often the open files and folders are checked.
const WATCH_INTERVAL: Duration = Duration::from_secs(2);

/// The stamps of the last check, by project-relative path (`""` is the project root).
pub(super) type Stamps = HashMap<String, Option<Stamp>>;

/// Modification time (nanoseconds since the epoch) and size.
type Stamp = (u128, u64);

/// A path, whether it is a folder, and its fresh stamp.
type Observed = (String, bool, Option<Stamp>);

impl Workbench {
    /// Starts checking the open project; the loop of the project before it ends.
    pub(super) fn start_watching(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.watch_stamps.clear();
        self.watch_task = Some(cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(WATCH_INTERVAL).await;
            let Ok(Some((project, files, folders))) = this.update(cx, |workbench, cx| {
                let project = workbench.project.clone()?;
                let files = workbench.editor.read(cx).paths();
                let mut folders = workbench.files.read(cx).expanded_folders();
                folders.push(String::new());
                Some((project, files, folders))
            }) else {
                return;
            };
            let paths: Vec<(String, bool)> = files
                .into_iter()
                .map(|path| (path, false))
                .chain(folders.into_iter().map(|path| (path, true)))
                .collect();
            let stamps = cx
                .background_executor()
                .spawn(async move {
                    paths
                        .into_iter()
                        .map(|(path, folder)| {
                            let stamp = project.stamp(&path).ok();
                            (path, folder, stamp)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let updated = this.update_in(cx, |workbench, window, cx| {
                workbench.apply_stamps(stamps, window, cx)
            });
            if updated.is_err() {
                return;
            }
        }));
    }

    /// Compares fresh stamps with the last ones: a changed folder is listed again, a changed
    /// clean file is reloaded when its text differs, a changed dirty one is only announced.
    fn apply_stamps(&mut self, stamps: Vec<Observed>, window: &mut Window, cx: &mut Context<Self>) {
        let mut folder_changed = false;
        let mut changed_files = Vec::new();
        for (path, folder, stamp) in stamps {
            let previous = self.watch_stamps.insert(path.clone(), stamp);
            // The first look only records.
            let Some(previous) = previous else {
                continue;
            };
            if previous == stamp || stamp.is_none() {
                continue;
            }
            if folder {
                folder_changed = true;
            } else {
                changed_files.push(path);
            }
        }
        if folder_changed {
            self.files.update(cx, |files, cx| files.refresh(cx));
        }
        let Some(project) = self.project.clone() else {
            return;
        };
        let dirty = self.editor.read(cx).dirty_paths();
        for path in changed_files {
            if dirty.contains(&path) {
                self.say(
                    format!("{path} diskte değişti; kaydedilmemiş değişiklikleriniz var"),
                    cx,
                );
                continue;
            }
            let Some(shown) = self.editor.read(cx).text_of(&path, cx) else {
                continue;
            };
            let project = project.clone();
            let read_path = path.clone();
            cx.spawn_in(window, async move |this, cx| {
                let document = cx
                    .background_executor()
                    .spawn(async move { project.read_document(&read_path) })
                    .await;
                let Ok(document) = document else {
                    return;
                };
                if document.content == shown {
                    return;
                }
                let _ = this.update_in(cx, |workbench, window, cx| {
                    // Typing since the read wins: a dirty tab is not reloaded.
                    if workbench.editor.read(cx).dirty_paths().contains(&path) {
                        return;
                    }
                    if let Some(project) = workbench.project.clone() {
                        let name = path.clone();
                        workbench
                            .editor
                            .update(cx, |editor, cx| editor.open_path(project, path, window, cx));
                        workbench.say(format!("{name} diskten yeniden yüklendi"), cx);
                    }
                });
            })
            .detach();
        }
    }
}
