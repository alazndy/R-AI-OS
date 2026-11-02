//! The regex replace bar of the open file.

use gpui::{AppContext, Context, Window};

use super::Workbench;
use crate::file_replace::{FileReplaceEvent, FileReplaceView};

impl Workbench {
    /// The open replace bar.
    pub fn file_replace(&self) -> Option<&gpui::Entity<FileReplaceView>> {
        self.file_replace.as_ref()
    }

    pub(super) fn open_file_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_replace.is_some() || self.editor.read(cx).path().is_none() {
            return;
        }
        let theme = self.theme();
        let view = cx.new(|cx| FileReplaceView::new(window, cx));
        view.update(cx, |view, cx| view.set_theme(theme, cx));
        self.file_replace_subscription = Some(cx.subscribe_in(
            &view,
            window,
            |this, view, event: &FileReplaceEvent, window, cx| match event {
                FileReplaceEvent::Changed { find, ignore_case } => {
                    let note = if find.is_empty() {
                        String::new()
                    } else {
                        match this.editor.read(cx).count_matches(find, *ignore_case, cx) {
                            Ok(0) => "Eşleşme yok".to_owned(),
                            Ok(count) => format!("{count} eşleşme"),
                            Err(error) => error.message,
                        }
                    };
                    view.update(cx, |view, cx| view.set_note(note, cx));
                }
                FileReplaceEvent::Apply {
                    find,
                    with,
                    ignore_case,
                } => {
                    let result = this.editor.update(cx, |editor, cx| {
                        editor.replace_matches(find, with, *ignore_case, window, cx)
                    });
                    match result {
                        Ok(count) => {
                            this.close_file_replace(window, cx);
                            this.say(format!("{count} yer değiştirildi"), cx);
                        }
                        Err(error) => {
                            view.update(cx, |view, cx| view.set_note(error.message, cx));
                        }
                    }
                }
                FileReplaceEvent::Close => this.close_file_replace(window, cx),
            },
        ));
        self.file_replace = Some(view);
        cx.notify();
    }

    /// Closes the bar and returns focus to the workbench.
    pub fn close_file_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_replace = None;
        self.file_replace_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }
}
