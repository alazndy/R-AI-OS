//! The source control view: git runs on the background executor, the view shows the result.

use std::path::{Path, PathBuf};

use gpui::{AppContext, Context, Window};
use rwk_core::error::Result;
use rwk_core::git;

use super::Workbench;
use crate::git::{GitEvent, GitView};

/// What one finished action asks the workbench to do besides showing the new status.
#[derive(Default)]
struct Aftermath {
    /// The commit went through: empty the message box.
    committed: bool,
    /// Working-tree files that were rewritten on disk; clean tabs showing them are reloaded.
    rewritten: Vec<String>,
}

impl Workbench {
    /// The open source control view.
    pub fn git_view(&self) -> Option<&gpui::Entity<GitView>> {
        self.git_view.as_ref()
    }

    /// Opens the source control view for the open project.
    pub(super) fn open_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        if self.git_view.is_some() {
            return;
        }
        let name = project.info().name.clone();
        let theme = self.theme();
        let view = cx.new(|cx| GitView::new(name, window, cx));
        view.update(cx, |view, cx| view.set_theme(theme, cx));
        self.git_subscription = Some(cx.subscribe_in(
            &view,
            window,
            |this, _view, event: &GitEvent, window, cx| {
                this.on_git_event(event.clone(), window, cx)
            },
        ));
        self.git_view = Some(view);
        self.git_job(|_| Ok(Aftermath::default()), window, cx);
        cx.notify();
    }

    /// Closes the view and returns focus to the workbench.
    pub fn close_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_view = None;
        self.git_subscription = None;
        // An action still running must not reach a future view.
        self.git_generation += 1;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn on_git_event(&mut self, event: GitEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            GitEvent::Close => self.close_git(window, cx),
            GitEvent::Refresh => self.git_job(|_| Ok(Aftermath::default()), window, cx),
            GitEvent::Open(path) => {
                self.close_git(window, cx);
                self.request_open_file(path, window, cx);
            }
            GitEvent::Stage(paths) => self.git_job(
                move |root| git::stage(root, &paths).map(|()| Aftermath::default()),
                window,
                cx,
            ),
            GitEvent::Unstage(paths) => self.git_job(
                move |root| git::unstage(root, &paths).map(|()| Aftermath::default()),
                window,
                cx,
            ),
            GitEvent::Discard(entries) => self.git_job(
                move |root| {
                    git::discard(root, &entries)?;
                    Ok(Aftermath {
                        committed: false,
                        rewritten: entries.into_iter().map(|entry| entry.path).collect(),
                    })
                },
                window,
                cx,
            ),
            GitEvent::Commit(message) => self.git_job(
                move |root| {
                    git::commit(root, &message, &rwk_core::handoff_input::looks_like_secret)?;
                    Ok(Aftermath {
                        committed: true,
                        rewritten: Vec::new(),
                    })
                },
                window,
                cx,
            ),
            GitEvent::Hunks { path, staged } => self.load_hunks(path, staged, window, cx),
            GitEvent::ApplyHunk {
                path,
                index,
                header,
                stage,
            } => self.git_job(
                move |root| {
                    git::apply_hunk(root, &path, index, &header, stage)
                        .map(|()| Aftermath::default())
                },
                window,
                cx,
            ),
            GitEvent::Switch(branch) => self.git_job(
                move |root| {
                    git::switch(root, &branch)?;
                    // Files differ from the other branch: every open clean tab is reloaded.
                    Ok(Aftermath {
                        committed: false,
                        rewritten: vec![String::new()],
                    })
                },
                window,
                cx,
            ),
        }
    }

    /// Reads the repository status in the background and marks changed files in the tree. A
    /// folder that is not a repository clears the marks; an older read is dropped.
    pub(super) fn refresh_git_badges(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        self.badge_generation += 1;
        let generation = self.badge_generation;
        let root: PathBuf = project.info().root.clone();
        // A folder without `.git` is no repository: nothing to mark, and no process to start.
        if !root.join(".git").exists() {
            self.files
                .update(cx, |files, cx| files.set_badges(Default::default(), cx));
            return;
        }
        cx.spawn(async move |this, cx| {
            let status = cx
                .background_executor()
                .spawn(async move { git::status(&root) })
                .await;
            this.update(cx, |workbench, cx| {
                if workbench.badge_generation != generation {
                    return;
                }
                let badges = status
                    .map(|status| git::badges(&status))
                    .unwrap_or_default();
                workbench
                    .files
                    .update(cx, |files, cx| files.set_badges(badges, cx));
            })
            .ok();
        })
        .detach();
    }

    /// Reads one file's diff on the background executor for the hunk panel.
    fn load_hunks(
        &mut self,
        path: String,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(view), Some(project)) = (self.git_view.clone(), self.project.clone()) else {
            return;
        };
        let root: PathBuf = project.info().root.clone();
        let generation = self.git_generation;
        cx.spawn_in(window, async move |this, cx| {
            let read = {
                let path = path.clone();
                cx.background_executor()
                    .spawn(async move { git::file_diff(&root, &path, staged) })
                    .await
            };
            this.update(cx, |workbench, cx| {
                if workbench.git_generation != generation {
                    return;
                }
                view.update(cx, |view, cx| match read {
                    Ok(diff) => view.set_hunks(&path, staged, diff, cx),
                    Err(error) => {
                        view.close_hunks(cx);
                        view.set_error(error.message, cx);
                    }
                });
            })
            .ok();
        })
        .detach();
    }

    /// Runs `action`, then reads the status and the branches, all on the background executor.
    /// An answer for a view that was closed in the meantime is dropped.
    fn git_job(
        &mut self,
        action: impl FnOnce(&Path) -> Result<Aftermath> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(view), Some(project)) = (self.git_view.clone(), self.project.clone()) else {
            return;
        };
        self.git_generation += 1;
        let generation = self.git_generation;
        view.update(cx, |view, cx| view.set_busy(cx));
        let root: PathBuf = project.info().root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let aftermath = action(&root);
                    // The status is read even after a failure, so the view shows the truth.
                    let state = git::status(&root).map(|status| {
                        let branches = git::branches(&root).unwrap_or_default();
                        (status, branches)
                    });
                    (aftermath, state)
                })
                .await;
            this.update_in(cx, |workbench, window, cx| {
                if workbench.git_generation != generation {
                    return;
                }
                let Some(view) = workbench.git_view.clone() else {
                    return;
                };
                let (aftermath, state) = outcome;
                workbench.refresh_branch(cx);
                workbench.refresh_git_badges(cx);
                let mut failure = None;
                match aftermath {
                    Ok(aftermath) => {
                        if aftermath.committed {
                            view.update(cx, |view, cx| view.clear_message(window, cx));
                        }
                        workbench.reload_clean_tabs(&aftermath.rewritten, window, cx);
                    }
                    Err(error) => failure = Some(error.message),
                }
                match state {
                    Ok((status, branches)) => {
                        view.update(cx, |view, cx| view.set_status(status, branches, cx));
                    }
                    Err(error) => {
                        failure.get_or_insert(error.message);
                    }
                }
                if let Some(message) = failure {
                    view.update(cx, |view, cx| view.set_error(message, cx));
                }
            })
            .ok();
        })
        .detach();
    }

    /// Reloads open tabs without edits whose file git rewrote. An empty path stands for every
    /// open tab (a branch switch).
    pub(super) fn reload_clean_tabs(
        &mut self,
        paths: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let dirty = self.editor.read(cx).dirty_paths();
        let open = self.editor.read(cx).paths();
        let all = paths.iter().any(String::is_empty);
        for path in open {
            if dirty.contains(&path) || !(all || paths.contains(&path)) {
                continue;
            }
            self.editor.update(cx, |editor, cx| {
                editor.open_path(project.clone(), path, window, cx)
            });
        }
    }
}
