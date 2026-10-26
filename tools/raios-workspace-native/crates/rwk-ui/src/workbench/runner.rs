//! Running the tasks of the user's task file in a terminal, and opening the place in the
//! project that a terminal's output points at.

use std::sync::Arc;

use gpui::{AppContext, Context, Window};
use rwk_core::runner::{last_project_location, RunTask, TaskSource};
use rwk_core::sessions::{SessionKind, SessionLaunch};
use rwk_core::workbench::quick_open::FileListing;

use super::palette::PaletteView;
use super::Workbench;

impl Workbench {
    /// Connects the task file (the application reads it from its config directory).
    pub fn set_task_source(&mut self, source: Option<Arc<dyn TaskSource>>) {
        self.task_source = source;
    }

    /// The tasks of the open project.
    pub fn tasks(&self) -> Vec<RunTask> {
        match (&self.task_source, &self.project) {
            (Some(source), Some(project)) => source.load(&project.info().root),
            _ => Vec::new(),
        }
    }

    /// Lists the tasks in the picker. With no tasks, says where to define them.
    pub(super) fn open_task_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_palette(window, cx);
        let tasks = self.tasks();
        if tasks.is_empty() {
            self.say(
                "Görev yok: raios-workspace-native/tasks.json dosyasına ekleyin",
                cx,
            );
            return;
        }
        let theme = self.theme();
        let palette = cx.new(|cx| PaletteView::for_files(window, cx));
        palette.update(cx, |palette, cx| {
            palette.set_theme(theme, cx);
            palette.set_files(
                FileListing {
                    files: tasks.into_iter().map(|task| task.name).collect(),
                    truncated: false,
                },
                cx,
            );
        });
        self.attach_palette(palette, window, cx);
        self.palette_picks_tasks = true;
    }

    /// Runs the task called `name` in a new terminal.
    pub(super) fn run_named_task(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.tasks().into_iter().find(|task| task.name == name) else {
            self.say("Görev bulunamadı", cx);
            return;
        };
        if self.spawn_terminal(SessionKind::Shell, SessionLaunch::Task(task), window, cx) {
            self.after_started_terminal(window, cx);
        }
    }

    /// Opens the file and line that the selected terminal's screen mentions last.
    pub(super) fn open_output_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(project), Some(pane)) = (self.project.clone(), self.terminals.get(self.selected))
        else {
            return;
        };
        let text = pane.view.read(cx).screen_text();
        let root = project.info().root.clone();
        let found = last_project_location(&text, &root, &|relative| {
            project.read_document(relative).is_ok()
        });
        match found {
            Some((path, line)) => self.request_open_file_at(path, Some(line), window, cx),
            None => self.say("Çıktıda proje dosyası konumu yok", cx),
        }
    }
}
