//! Opening the project search view and running searches on the background executor.

use std::sync::Arc;

use gpui::{AppContext, Context, Window};
use rwk_core::search::SearchSource;

use super::Workbench;
use crate::search::{SearchEvent, SearchView};

impl Workbench {
    /// Connects the search source (the RAIOS CLI by default).
    pub fn set_search_source(&mut self, source: Option<Arc<dyn SearchSource>>) {
        self.search_source = source;
    }

    /// The open search view.
    pub fn search(&self) -> Option<&gpui::Entity<SearchView>> {
        self.search.as_ref()
    }

    /// Opens the search view for the open project.
    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        if self.search.is_some() {
            return;
        }
        let name = project.info().name.clone();
        let theme = self.theme();
        let view = cx.new(|cx| SearchView::new(name, window, cx));
        view.update(cx, |view, cx| view.set_theme(theme, cx));
        self.search_subscription = Some(cx.subscribe_in(
            &view,
            window,
            |this, _view, event: &SearchEvent, window, cx| match event {
                SearchEvent::Run(pattern, ignore_case) => {
                    this.run_search(pattern.clone(), *ignore_case, cx);
                }
                SearchEvent::Open(path, line) => {
                    // The overlay would hide the file it just opened; closing returns the floor.
                    this.close_search(window, cx);
                    this.request_open_file_at(path.clone(), Some(*line), window, cx);
                }
                SearchEvent::Replace {
                    find,
                    with,
                    ignore_case,
                    apply,
                } => this.replace_in_hits(find, with, *ignore_case, *apply, window, cx),
                SearchEvent::Close => this.close_search(window, cx),
            },
        ));
        self.search = Some(view);
        cx.notify();
    }

    /// Previews or performs a literal replacement in the files of the hits on screen. Files
    /// with unsaved edits are left alone and named; open clean tabs are reloaded afterwards.
    fn replace_in_hits(
        &mut self,
        find: &str,
        with: &str,
        ignore_case: bool,
        apply: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(view), Some(project)) = (self.search.clone(), self.project.clone()) else {
            return;
        };
        let replacement = match rwk_core::replace::Replacement::new(find, with, ignore_case) {
            Ok(replacement) => replacement,
            Err(error) => {
                view.update(cx, |view, cx| {
                    view.set_replace_note(error.message, false, cx)
                });
                return;
            }
        };
        let paths: Vec<String> = view
            .read(cx)
            .result()
            .map(|result| result.hits.iter().map(|hit| hit.path.clone()).collect())
            .unwrap_or_default();
        let busy: std::collections::HashSet<String> =
            self.editor.read(cx).dirty_paths().into_iter().collect();
        self.search_generation += 1;
        let generation = self.search_generation;
        let pattern = find.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let report = cx
                .background_executor()
                .spawn({
                    let paths = paths.clone();
                    async move {
                        if apply {
                            rwk_core::replace::replace_all(&project, &paths, &replacement, &busy)
                        } else {
                            rwk_core::replace::preview(&project, &paths, &replacement, &busy)
                        }
                    }
                })
                .await;
            this.update_in(cx, |workbench, window, cx| {
                if workbench.search_generation != generation {
                    return;
                }
                let Some(view) = workbench.search.clone() else {
                    return;
                };
                let mut note = format!(
                    "{} dosyada {} yer",
                    report.files.len(),
                    report.replacements()
                );
                if !report.skipped.is_empty() {
                    note.push_str(&format!(
                        " · kaydedilmemiş, atlanan: {}",
                        report.skipped.join(", ")
                    ));
                }
                if !report.failed.is_empty() {
                    note.push_str(&format!(
                        " · okunamadı/yazılamadı: {}",
                        report.failed.join(", ")
                    ));
                }
                let nothing = report.files.is_empty();
                if apply {
                    note.push_str(" değiştirildi");
                    let changed: Vec<String> =
                        report.files.iter().map(|(path, _)| path.clone()).collect();
                    view.update(cx, |view, cx| view.set_replace_note(note, false, cx));
                    workbench.reload_clean_tabs(&changed, window, cx);
                    workbench.run_search(pattern, ignore_case, cx);
                } else if nothing {
                    note.push_str(" — değişecek bir şey yok");
                    view.update(cx, |view, cx| view.set_replace_note(note, false, cx));
                } else {
                    note.push_str(" değişecek; onaylamak için düğmeye tekrar basın");
                    view.update(cx, |view, cx| view.set_replace_note(note, true, cx));
                }
            })
            .ok();
        })
        .detach();
    }

    /// Closes the search view and returns focus to the workbench.
    pub fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        self.search_subscription = None;
        // A search that is still running must not reach a future view.
        self.search_generation += 1;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Runs one search on the background executor; an answer for a superseded search is dropped.
    pub(super) fn run_search(
        &mut self,
        pattern: String,
        ignore_case: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.search.clone() else {
            return;
        };
        let Some(project) = self.project.clone() else {
            return;
        };
        let Some(source) = self.search_source.clone() else {
            view.update(cx, |view, cx| {
                view.set_failure("Arama kaynağı yapılandırılmadı.".to_owned(), cx)
            });
            return;
        };
        self.search_generation += 1;
        let generation = self.search_generation;
        view.update(cx, |view, cx| view.set_loading(cx));
        let root = project.info().root.clone();
        cx.spawn(async move |this, cx| {
            let located = cx
                .background_executor()
                .spawn(async move { source.locate(&root, &pattern, ignore_case) })
                .await;
            this.update(cx, |workbench, cx| {
                if workbench.search_generation != generation {
                    return;
                }
                let Some(view) = workbench.search.clone() else {
                    return;
                };
                view.update(cx, |view, cx| match located {
                    Ok(result) => view.set_result(result, cx),
                    Err(error) => view.set_failure(error.message, cx),
                });
            })
            .ok();
        })
        .detach();
    }
}
