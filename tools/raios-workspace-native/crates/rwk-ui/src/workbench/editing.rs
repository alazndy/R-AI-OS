//! Editing preferences of this session (auto-save, format on save) and the find box.

use gpui::{Context, Window};

use super::Workbench;

impl Workbench {
    /// Whether changed files are saved when the window loses focus.
    pub fn auto_save(&self) -> bool {
        self.auto_save
    }

    /// Whether saving formats the file first.
    pub fn format_on_save(&self) -> bool {
        self.format_on_save
    }

    /// Saves the active file; with format on save the language server formats it first.
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let format = self.format_on_save;
        self.editor.update(cx, |editor, cx| {
            if format {
                editor.save_formatted(project, window, cx);
            } else {
                editor.save(project, cx);
            }
        });
    }

    /// Sets the switches without announcing them (the application restores stored ones).
    pub fn set_editing_preferences(&mut self, auto_save: bool, format_on_save: bool) {
        self.auto_save = auto_save;
        self.format_on_save = format_on_save;
    }

    fn preferences_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(super::WorkbenchEvent::PreferencesChanged {
            auto_save: self.auto_save,
            format_on_save: self.format_on_save,
        });
    }

    pub(super) fn toggle_auto_save(&mut self, cx: &mut Context<Self>) {
        self.auto_save = !self.auto_save;
        let state = if self.auto_save { "açık" } else { "kapalı" };
        self.preferences_changed(cx);
        self.say(format!("Otomatik kaydetme {state}"), cx);
    }

    pub(super) fn toggle_format_on_save(&mut self, cx: &mut Context<Self>) {
        self.format_on_save = !self.format_on_save;
        let state = if self.format_on_save {
            "açık"
        } else {
            "kapalı"
        };
        self.preferences_changed(cx);
        self.say(format!("Kaydederken biçimlendirme {state}"), cx);
    }

    /// Saves every changed tab without asking; a tab whose file changed on disk is left alone
    /// (its edits stay, and the editor says so).
    pub(super) fn auto_save_now(&mut self, cx: &mut Context<Self>) {
        if !self.auto_save {
            return;
        }
        let Some(project) = self.project.clone() else {
            return;
        };
        let dirty = self.editor.read(cx).dirty_paths();
        for path in dirty {
            let project = project.clone();
            self.editor
                .update(cx, |editor, cx| editor.save_path(&path, project, cx));
        }
    }

    pub(super) fn find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.open_find(replace, window, cx));
    }
}
