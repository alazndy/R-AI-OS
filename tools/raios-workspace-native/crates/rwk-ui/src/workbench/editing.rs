//! Editing preferences of this session (auto-save, format on save) and the find box.

use gpui::{AppContext, Context, Window};

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
            toolbar: self.toolbar_layout,
            editor: self.editor_prefs,
        });
    }

    /// Sets the stored toolbar layout without announcing it.
    pub fn set_toolbar_layout(&mut self, layout: rwk_core::workbench::ToolbarLayout) {
        self.toolbar_layout = layout;
    }

    /// The stored editor settings, applied before any tab or terminal exists.
    pub fn set_editor_prefs(
        &mut self,
        prefs: rwk_core::workbench::EditorPrefs,
        cx: &mut Context<Self>,
    ) {
        let prefs = prefs.clamped();
        self.editor_prefs = prefs;
        self.editor.update(cx, |editor, _| editor.init_prefs(prefs));
    }

    /// Text size, tab width and wrapping.
    pub fn editor_prefs(&self) -> rwk_core::workbench::EditorPrefs {
        self.editor_prefs
    }

    /// Applies new editor settings to every tab and terminal and stores them.
    pub(super) fn change_editor_prefs(
        &mut self,
        prefs: rwk_core::workbench::EditorPrefs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prefs = prefs.clamped();
        if prefs == self.editor_prefs {
            return;
        }
        self.editor_prefs = prefs;
        self.editor
            .update(cx, |editor, cx| editor.set_prefs(prefs, window, cx));
        let size = f32::from(prefs.font_size);
        for pane in &self.terminals {
            pane.view
                .update(cx, |view, cx| view.set_font_size(size, cx));
        }
        self.preferences_changed(cx);
        cx.notify();
    }

    pub(super) fn zoom(&mut self, delta: i8, window: &mut Window, cx: &mut Context<Self>) {
        let prefs = self.editor_prefs.zoomed(delta);
        self.change_editor_prefs(prefs, window, cx);
        let size = self.editor_prefs.font_size;
        self.say(format!("Yazı boyutu {size}"), cx);
    }

    /// Asks for a line to go to in the open file.
    pub(super) fn open_go_to_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::rename::{PromptPurpose, RenamePromptEvent, RenameTerminalPrompt};
        if self.rename_prompt.is_some() {
            return;
        }
        let theme = self.theme();
        let prompt =
            cx.new(|cx| RenameTerminalPrompt::entry(PromptPurpose::GoToLine, "", window, cx));
        prompt.update(cx, |prompt, cx| prompt.set_theme(theme, cx));
        self.rename_subscription = Some(cx.subscribe_in(
            &prompt,
            window,
            |this, _prompt, event: &RenamePromptEvent, window, cx| match event {
                RenamePromptEvent::EntryName(target) => {
                    this.close_rename_prompt(window, cx);
                    if let Some((line, column)) = rwk_core::workbench::parse_line_target(target) {
                        this.editor
                            .update(cx, |editor, cx| editor.go_to_line(line, column, window, cx));
                    }
                }
                RenamePromptEvent::Close => this.close_rename_prompt(window, cx),
                RenamePromptEvent::NewTask(_) | RenamePromptEvent::Rename(_) => {}
            },
        ));
        self.rename_prompt = Some(prompt);
        cx.notify();
    }

    /// What the toolbar shows and how.
    pub fn toolbar_layout(&self) -> rwk_core::workbench::ToolbarLayout {
        self.toolbar_layout
    }

    /// Replaces the toolbar layout, repaints and stores it.
    pub(super) fn change_toolbar(
        &mut self,
        layout: rwk_core::workbench::ToolbarLayout,
        cx: &mut Context<Self>,
    ) {
        if layout == self.toolbar_layout {
            return;
        }
        self.toolbar_layout = layout;
        self.preferences_changed(cx);
        cx.notify();
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
