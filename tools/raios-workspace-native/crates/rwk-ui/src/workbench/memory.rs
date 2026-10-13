//! Opening the memory view and feeding it from `memory.md` and the record source.

use std::sync::Arc;

use gpui::{AppContext, Context, Window};
use rwk_core::error::ErrorCode;
use rwk_core::memory::MemorySource;

use super::Workbench;
use crate::memory::{Loaded, MemoryEvent, MemoryView};

impl Workbench {
    /// Connects the structured record source (read-only `workspace.db`).
    pub fn set_memory_source(&mut self, source: Option<Arc<dyn MemorySource>>) {
        self.memory_source = source;
    }

    /// The open memory view.
    pub fn memory(&self) -> Option<&gpui::Entity<MemoryView>> {
        self.memory.as_ref()
    }

    /// Opens the memory view for the open project and starts reading both sources.
    pub(super) fn open_memory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        if self.memory.is_some() {
            return;
        }
        let name = project.info().name.clone();
        let theme = self.theme();
        let view = cx.new(|cx| MemoryView::new(name, window, cx));
        view.update(cx, |view, cx| view.set_theme(theme, cx));
        self.memory_subscription = Some(cx.subscribe_in(
            &view,
            window,
            |this, _view, event: &MemoryEvent, window, cx| match event {
                MemoryEvent::Close => this.close_memory(window, cx),
                MemoryEvent::Refresh => this.load_memory(cx),
            },
        ));
        self.memory = Some(view);
        self.load_memory(cx);
        cx.notify();
    }

    /// Closes the memory view and returns focus to the workbench.
    pub fn close_memory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.memory = None;
        self.memory_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Reads `memory.md` and the records on the background executor.
    pub(super) fn load_memory(&mut self, cx: &mut Context<Self>) {
        let (Some(view), Some(project)) = (self.memory.clone(), self.project.clone()) else {
            return;
        };
        view.update(cx, |view, cx| view.set_loading(cx));
        let source = self.memory_source.clone();
        cx.spawn(async move |_this, cx| {
            let for_document = Arc::clone(&project);
            let document = cx
                .background_executor()
                .spawn(async move { for_document.read_document("memory.md") })
                .await;
            let document = match document {
                Ok(document) => Loaded::Ready(Some(document.content)),
                Err(error) if error.code == ErrorCode::NotFound => Loaded::Ready(None),
                Err(error) => Loaded::Failed(error.message),
            };
            view.update(cx, |view, cx| view.set_document(document, cx));
            let records = match source {
                None => Loaded::Failed("Hafıza veritabanı yapılandırılmadı.".to_owned()),
                Some(source) => {
                    let result = cx
                        .background_executor()
                        .spawn(async move { source.snapshot(&project) })
                        .await;
                    match result {
                        Ok(snapshot) => Loaded::Ready(snapshot),
                        Err(error) => Loaded::Failed(error.message),
                    }
                }
            };
            view.update(cx, |view, cx| view.set_records(records, cx));
        })
        .detach();
    }
}
