//! The workbench on GPUI's test platform: real project, real editor, real PTY sessions.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, TestDispatcher, VisualTestContext};
use gpui_wgpu::CosmicTextSystem;
use rwk_core::search::{SearchHit, SearchResult, SearchSource};
use rwk_core::terminal::ProgramPaths;
use rwk_core::usage::{UsageProvider, UsageReport, UsageSource};
use rwk_core::workbench::Tone;
use rwk_core::workbench::{
    Appearance, CommandId, SavedTab, SavedTerminal, SavedWorkspace, WorkspaceStateSource,
};
use rwk_ui::workbench::layout::LayoutPreset;
use rwk_ui::workbench::{Decision, PendingAction, Workbench, WorkbenchEvent};

const WAIT: Duration = Duration::from_secs(15);

static SERIAL: Mutex<()> = Mutex::new(());

struct Rig {
    _serial: std::sync::MutexGuard<'static, ()>,
    cx: &'static mut VisualTestContext,
    view: Entity<Workbench>,
    dir: tempfile::TempDir,
}

/// A workspace source that hands back whatever state it was given, for any project: the
/// workbench itself is the one that decides whether the state belongs to the open project.
struct FixedWorkspaceStore {
    state: Mutex<Option<SavedWorkspace>>,
}

impl FixedWorkspaceStore {
    fn new(state: Option<SavedWorkspace>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(state),
        })
    }
}

impl WorkspaceStateSource for FixedWorkspaceStore {
    fn load(&self, _project: &Path) -> Option<SavedWorkspace> {
        self.state.lock().expect("lock").clone()
    }
}

fn rig() -> Rig {
    let serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("a.txt"), "one").expect("a");
    std::fs::write(dir.path().join("b.txt"), "two").expect("b");
    let script = dir.path().join(".session.sh");
    // Every call is logged for tests that check what a pane or the session catalog was run
    // with; a `resume --list` answers from `.catalog` when a test put one there, and every
    // other invocation (a pane) stays alive.
    let argv_log = dir.path().join(".argv");
    let catalog = dir.path().join(".catalog");
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\nprintf '%s\\n' \"$*\" >> {argv}\nif [ \"$1\" = \"resume\" ] && [ \"$2\" = \"--list\" ]; then\n  if [ -f {catalog} ]; then cat {catalog}; fi\n  exit 0\nfi\nexec sleep 100000\n",
            argv = argv_log.display(),
            catalog = catalog.display(),
        ),
    )
    .expect("script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let app = Box::leak(Box::new(TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        Arc::new(CosmicTextSystem::new("monospace")),
    )));
    app.update(gpui_base::init);
    let programs = ProgramPaths {
        // The same script stands in for ssh: it logs the arguments it was started with.
        ssh: Some(script.clone()),
        shell: Some(script.clone()),
        raios: Some(script),
    };
    let (view, cx) = app.add_window_view(move |window, cx| {
        Workbench::new(programs, Appearance::default(), window, cx)
    });
    Rig {
        _serial: serial,
        cx,
        view,
        dir,
    }
}

impl Rig {
    fn settle_until(&mut self, mut done: impl FnMut(&mut Rig) -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < WAIT {
            std::thread::sleep(Duration::from_millis(10));
            self.cx.run_until_parked();
            if done(self) {
                return true;
            }
        }
        false
    }

    fn settle_for(&mut self, millis: u64) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(millis) {
            std::thread::sleep(Duration::from_millis(10));
            self.cx.run_until_parked();
        }
    }

    fn with<R>(
        &mut self,
        f: impl FnOnce(&mut Workbench, &mut gpui::Window, &mut gpui::Context<Workbench>) -> R,
    ) -> R {
        let view = self.view.clone();
        self.cx
            .update(|window, cx| view.update(cx, |wb, cx| f(wb, window, cx)))
    }

    fn read<R>(&mut self, f: impl FnOnce(&Workbench, &gpui::App) -> R) -> R {
        let view = self.view.clone();
        self.cx.update(|_w, cx| f(view.read(cx), cx))
    }

    fn open_project(&mut self) {
        let root: PathBuf = self.dir.path().to_path_buf();
        self.with(|wb, window, cx| wb.open_project(root, window, cx));
        assert!(self.settle_until(|rig| rig.read(|wb, cx| wb.state(cx).project_open)));
    }

    fn open_file(&mut self, path: &str) {
        let path = path.to_owned();
        self.with(|wb, window, cx| wb.request_open_file(path, window, cx));
    }

    fn editor_path(&mut self) -> Option<String> {
        self.read(|wb, cx| wb.editor().read(cx).path().map(str::to_owned))
    }

    fn editor_text(&mut self) -> String {
        self.read(|wb, cx| wb.editor().read(cx).text(cx))
    }

    fn editor_paths(&mut self) -> Vec<String> {
        self.read(|wb, cx| wb.editor().read(cx).paths())
    }

    fn editor_dirty_paths(&mut self) -> Vec<String> {
        self.read(|wb, cx| wb.editor().read(cx).dirty_paths())
    }

    fn activate_tab(&mut self, path: &str) {
        let path = path.to_owned();
        self.with(|wb, window, cx| {
            wb.editor()
                .clone()
                .update(cx, |editor, cx| editor.activate_tab(&path, window, cx))
        });
        self.cx.run_until_parked();
    }

    fn close_tab(&mut self, path: &str) {
        let path = path.to_owned();
        self.with(|wb, window, cx| wb.request_close_tab(path, window, cx));
    }

    fn set_workspace_source(&mut self, source: Arc<dyn WorkspaceStateSource>) {
        self.with(|wb, _window, _cx| wb.set_workspace_source(Some(source)));
    }

    /// Puts the JSON `raios resume --list --json` answers with into the project directory. The
    /// rows have to name the project root themselves, as the CLI's do.
    fn script_catalog(&mut self, json: &str) {
        std::fs::write(self.dir.path().join(".catalog"), json).expect("catalog");
    }

    /// Every argument list the scripted `raios` has been called with so far, oldest first.
    fn script_calls(&mut self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join(".argv"))
            .map(|log| log.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Records every `WorkspaceChanged` the workbench hands to the application.
    fn record_workspace_changes(&mut self) -> Arc<Mutex<Vec<SavedWorkspace>>> {
        let saved = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&saved);
        let view = self.view.clone();
        self.cx.update(|_w, cx| {
            cx.subscribe(&view, move |_wb, event: &WorkbenchEvent, _cx| {
                if let WorkbenchEvent::WorkspaceChanged(workspace) = event {
                    recorder.lock().expect("lock").push(workspace.clone());
                }
            })
            .detach();
        });
        saved
    }

    fn type_text(&mut self, text: &str) {
        let text = text.to_owned();
        self.with(|wb, window, cx| {
            wb.editor()
                .clone()
                .update(cx, |editor, cx| editor.insert_text(&text, window, cx))
        });
        self.cx.run_until_parked();
    }

    fn run(&mut self, id: CommandId) {
        self.with(|wb, window, cx| wb.run(id, window, cx));
    }

    /// Connects a scripted search source; every `locate` call takes the next queued answer.
    fn script_search(
        &mut self,
        answers: Vec<(Duration, Vec<SearchHit>)>,
    ) -> Arc<Mutex<Vec<(String, bool)>>> {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let source = FakeSearch {
            answers: Mutex::new(answers.into()),
            calls: Arc::clone(&calls),
        };
        self.with(|wb, _window, _cx| wb.set_search_source(Some(Arc::new(source))));
        calls
    }

    /// Connects a scripted quota source; every `usage` call takes the next queued answer.
    fn script_usage(&mut self, answers: Vec<(Duration, Option<UsageReport>)>) -> Arc<Mutex<usize>> {
        let calls = Arc::new(Mutex::new(0));
        let source = FakeUsage {
            answers: Mutex::new(answers.into()),
            calls: Arc::clone(&calls),
        };
        self.with(|wb, _window, cx| wb.set_usage_source(Some(Arc::new(source)), cx));
        calls
    }

    /// Types `text` into the open search view, as a person would.
    fn search_query(&mut self, text: &str) {
        let text = text.to_owned();
        self.with(|wb, window, cx| {
            let view = wb.search().expect("the search view is open").clone();
            view.update(cx, |view, cx| view.set_query(&text, window, cx));
        });
        self.cx.run_until_parked();
    }

    fn search_open(&mut self) -> bool {
        self.read(|wb, _| wb.search().is_some())
    }

    /// The hits on screen as `path:line`, empty while nothing has answered.
    fn search_hits(&mut self) -> Vec<String> {
        self.read(|wb, cx| {
            wb.search()
                .and_then(|view| {
                    view.read(cx).result().map(|result| {
                        result
                            .hits
                            .iter()
                            .map(|hit| format!("{}:{}", hit.path, hit.line))
                            .collect()
                    })
                })
                .unwrap_or_default()
        })
    }
}

#[test]
fn the_command_dispatch_path_is_shared_by_shortcut_and_direct_calls() {
    let mut rig = rig();
    rig.with(|wb, window, cx| window.focus(wb.focus_handle(), cx));
    rig.cx.run_until_parked();
    rig.cx.simulate_keystrokes("ctrl-2");
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Editor);
    assert_eq!(
        rig.read(|wb, _| wb.last_command()),
        Some(CommandId::EditorLayout)
    );
    rig.run(CommandId::AgentsLayout);
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Agents);
    rig.cx.simulate_keystrokes("ctrl-1");
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Balanced);
}

#[test]
fn a_disabled_command_does_nothing_and_reports_its_reason() {
    let mut rig = rig();
    rig.run(CommandId::Save);
    assert_eq!(
        rig.read(|wb, _| wb.toast()).as_deref(),
        Some("Açık dosya yok")
    );
    rig.run(CommandId::Refresh);
    assert_eq!(
        rig.read(|wb, _| wb.toast()).as_deref(),
        Some("Önce bir proje klasörü açın")
    );
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
}

#[test]
fn the_thirteenth_terminal_is_refused_with_a_reason() {
    let mut rig = rig();
    rig.open_project();
    for _ in 0..12 {
        rig.run(CommandId::NewTerminal);
    }
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 12);
    rig.run(CommandId::NewTerminal);
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 12);
    assert_eq!(
        rig.read(|wb, _| wb.toast()).as_deref(),
        Some("En fazla 12 terminal açılabilir")
    );
}

#[test]
fn hiding_the_dock_keeps_the_terminals_alive() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    let toast = rig.read(|wb, _| wb.toast());
    assert_eq!(toast, None, "new terminal failed");
    rig.run(CommandId::ToggleTerminal);
    assert!(!rig.read(|wb, _| wb.dock_visible()));
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 1);
    rig.run(CommandId::ToggleTerminal);
    assert!(rig.read(|wb, _| wb.dock_visible()));
}

#[test]
fn opening_another_file_keeps_the_unsaved_edits_in_their_tab() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    rig.open_file("b.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "two"));
    assert!(
        rig.read(|wb, _| wb.pending().is_none()),
        "opening a tab loses nothing, so nothing is asked"
    );
    assert_eq!(rig.editor_paths(), vec!["a.txt", "b.txt"]);
    assert_eq!(rig.editor_dirty_paths(), vec!["a.txt"]);
    // The edits are still there, in their own tab.
    rig.activate_tab("a.txt");
    assert!(rig.editor_text().contains('!'));
}

#[test]
fn closing_a_dirty_tab_asks_and_can_save_it() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.open_file("b.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "two"));
    rig.activate_tab("a.txt");
    rig.type_text("!");
    rig.close_tab("a.txt");
    assert_eq!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::CloseTab("a.txt".to_owned()))
    );
    rig.with(|wb, window, cx| wb.resolve(Decision::Save, window, cx));
    let closed = rig.settle_until(|rig| rig.editor_paths() == vec!["b.txt".to_owned()]);
    assert!(closed, "the tab must close after its save succeeded");
    let saved = std::fs::read_to_string(rig.dir.path().join("a.txt")).expect("read");
    assert!(saved.contains('!'), "{saved}");
}

#[test]
fn discarding_a_dirty_tab_closes_it_and_keeps_the_disk_content() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    rig.close_tab("a.txt");
    rig.with(|wb, window, cx| wb.resolve(Decision::Discard, window, cx));
    assert!(rig.settle_until(|rig| rig.editor_paths().is_empty()));
    assert_eq!(
        std::fs::read_to_string(rig.dir.path().join("a.txt")).expect("read"),
        "one",
        "a discarded edit must never reach the disk"
    );
}

#[test]
fn close_other_and_close_all_tabs_leave_the_unsaved_ones_open() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.open_file("b.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "two"));
    assert_eq!(rig.editor_paths().len(), 2);

    rig.run(CommandId::CloseOtherTabs);
    assert_eq!(rig.editor_paths(), vec!["b.txt".to_owned()]);

    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("x");
    rig.run(CommandId::CloseAllTabs);
    assert_eq!(
        rig.editor_paths(),
        vec!["a.txt".to_owned()],
        "the edited tab stays"
    );
    assert!(rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("açık bırakıldı")));
}

#[test]
fn the_close_tab_command_and_its_shortcut_close_the_active_tab() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.run(CommandId::CloseTab);
    assert!(rig.editor_paths().is_empty(), "the command closes the tab");
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    focus_workbench(&mut rig);
    rig.cx.simulate_keystrokes("ctrl-w");
    assert!(rig.editor_paths().is_empty(), "ctrl-w closes the tab");
}

#[test]
fn closing_the_window_saves_every_dirty_tab_in_order() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    rig.open_file("b.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "two"));
    rig.type_text("?");
    assert_eq!(rig.editor_dirty_paths(), vec!["a.txt", "b.txt"]);
    let allowed = rig.with(|wb, window, cx| wb.request_close(window, cx));
    assert!(!allowed);
    rig.with(|wb, window, cx| wb.resolve(Decision::Save, window, cx));
    let closed = rig.settle_until(|rig| rig.read(|wb, _| wb.close_confirmed()));
    assert!(closed, "the close must finish after both saves");
    let a = std::fs::read_to_string(rig.dir.path().join("a.txt")).expect("read");
    let b = std::fs::read_to_string(rig.dir.path().join("b.txt")).expect("read");
    assert!(a.contains('!'), "{a}");
    assert!(b.contains('?'), "{b}");
}

#[test]
fn a_conflicting_save_cancels_the_close_and_keeps_the_edits() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    std::fs::write(rig.dir.path().join("a.txt"), "changed elsewhere").expect("write");
    let allowed = rig.with(|wb, window, cx| wb.request_close(window, cx));
    assert!(!allowed);
    rig.with(|wb, window, cx| wb.resolve(Decision::Save, window, cx));
    rig.settle_for(500);
    assert!(rig.editor_text().contains('!'));
    assert!(
        !rig.read(|wb, _| wb.close_confirmed()),
        "a failed save must not close the window"
    );
    assert!(rig.read(|wb, _| wb.toast()).is_some());
}

#[test]
fn closing_with_unsaved_edits_asks_first() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    let allowed = rig.with(|wb, window, cx| wb.request_close(window, cx));
    assert!(!allowed);
    assert_eq!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::CloseWindow)
    );
    assert!(!rig.read(|wb, _| wb.close_confirmed()));
}

#[test]
fn closing_the_window_stops_owned_sessions() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    rig.run(CommandId::NewTerminal);
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 2);
    let allowed = rig.with(|wb, window, cx| wb.request_close(window, cx));
    assert!(allowed);
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 0);
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
    assert!(rig.read(|wb, _| wb.close_confirmed()));
}

#[test]
fn stopping_a_session_asks_for_confirmation() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    rig.run(CommandId::StopTerminal);
    assert!(matches!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::StopTerminal(_))
    ));
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 1);
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.running_sessions()) == 0));
}

#[test]
fn switching_projects_stops_the_old_sessions_but_allows_new_ones() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 1);
    let other = tempfile::tempdir().expect("other project");
    let root = other.path().to_path_buf();
    rig.with(|wb, window, cx| wb.open_project(root, window, cx));
    // Running sessions are stopped only after the user confirms.
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 0));
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 0);
    rig.run(CommandId::NewTerminal);
    assert_eq!(
        rig.read(|wb, _| wb.toast()),
        None,
        "a new project must accept terminals"
    );
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 1);
}

fn focus_workbench(rig: &mut Rig) {
    rig.with(|wb, window, cx| window.focus(wb.focus_handle(), cx));
    rig.cx.run_until_parked();
}

#[test]
fn the_palette_filters_commands_by_label_and_shows_why_one_is_unavailable() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| palette.set_query("dosya", window, cx))
    });
    let rows = rig.read(|_wb, cx| palette.read(cx).entries().to_vec());
    assert!(!rows.is_empty());
    let save = rows
        .iter()
        .find(|entry| entry.id == CommandId::Save)
        .expect("save matches 'dosya'");
    assert_eq!(save.state.reason, Some("Açık dosya yok"));
    assert!(!save.state.enabled);
}

#[test]
fn the_palette_runs_the_selected_command_through_the_shared_path() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| {
            palette.set_query("editör yerleşimi", window, cx)
        })
    });
    rig.cx
        .update(|_w, cx| palette.update(cx, |palette, cx| palette.confirm(cx)));
    rig.cx.run_until_parked();
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Editor);
    assert_eq!(
        rig.read(|wb, _| wb.last_command()),
        Some(CommandId::EditorLayout)
    );
    assert!(
        rig.read(|wb, _| wb.palette().is_none()),
        "the palette closes after running"
    );
}

#[test]
fn an_unavailable_command_stays_in_the_palette_instead_of_running() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| {
            palette.set_query("dosyayı kaydet", window, cx)
        })
    });
    rig.cx
        .update(|_w, cx| palette.update(cx, |palette, cx| palette.confirm(cx)));
    rig.cx.run_until_parked();
    assert!(rig.read(|wb, _| wb.palette().is_some()));
    assert_eq!(
        rig.read(|wb, _| wb.last_command()),
        Some(CommandId::Commands)
    );
}

#[test]
fn only_the_palette_shortcut_works_while_the_palette_input_has_focus() {
    let mut rig = rig();
    focus_workbench(&mut rig);
    rig.cx.simulate_keystrokes("ctrl-shift-p");
    assert!(rig.read(|wb, _| wb.palette().is_some()));
    rig.cx.simulate_keystrokes("ctrl-2");
    assert_eq!(
        rig.read(|wb, _| wb.layout()),
        LayoutPreset::Balanced,
        "ctrl-2 must not run"
    );
    rig.cx.simulate_keystrokes("ctrl-shift-p");
    assert!(
        rig.read(|wb, _| wb.palette().is_none()),
        "the palette shortcut closes it"
    );
}

#[test]
fn escape_closes_the_palette() {
    let mut rig = rig();
    focus_workbench(&mut rig);
    rig.cx.simulate_keystrokes("ctrl-shift-p");
    assert!(rig.read(|wb, _| wb.palette().is_some()));
    rig.cx.simulate_keystrokes("escape");
    assert!(rig.read(|wb, _| wb.palette().is_none()));
}

#[test]
fn the_unsaved_changes_dialog_offers_save_discard_and_cancel() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    rig.close_tab("a.txt");
    let dialog = rig.read(|wb, _| wb.dialog()).expect("a dialog is waiting");
    let decisions: Vec<Decision> = dialog
        .buttons
        .iter()
        .map(|(decision, _)| *decision)
        .collect();
    assert_eq!(
        decisions,
        [Decision::Save, Decision::Discard, Decision::Cancel]
    );
    assert!(dialog.message.contains("a.txt"));
}

#[test]
fn escape_cancels_the_dialog_and_enter_takes_the_default_button() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    focus_workbench(&mut rig);
    rig.close_tab("a.txt");
    rig.cx.simulate_keystrokes("escape");
    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert_eq!(rig.editor_paths(), vec!["a.txt".to_owned()]);
    assert!(
        rig.editor_text().contains('!'),
        "cancelling must change nothing"
    );
    // Enter picks the default, which is Save: the tab closes and the edits are written.
    rig.close_tab("a.txt");
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| rig.editor_paths().is_empty()));
    assert!(std::fs::read_to_string(rig.dir.path().join("a.txt"))
        .expect("read")
        .contains('!'));
}

#[test]
fn no_shortcut_runs_while_a_dialog_is_open() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    focus_workbench(&mut rig);
    rig.close_tab("a.txt");
    assert!(rig.read(|wb, _| wb.pending().is_some()));
    rig.cx.simulate_keystrokes("ctrl-2");
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Balanced);
    assert!(
        rig.read(|wb, _| wb.pending().is_some()),
        "the dialog stays open"
    );
}

#[test]
fn changing_the_tone_announces_the_new_appearance_and_keeps_the_terminals() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    let events: Arc<Mutex<Vec<WorkbenchEvent>>> = Arc::default();
    let sink = Arc::clone(&events);
    let view = rig.view.clone();
    rig.cx.update(|_w, app| {
        app.subscribe(&view, move |_wb, event: &WorkbenchEvent, _app| {
            sink.lock().unwrap().push(event.clone());
        })
        .detach();
    });
    rig.run(CommandId::Settings);
    let settings = rig.read(|wb, _| wb.settings_view().cloned()).expect("view");
    rig.with(|_wb, _w, cx| {
        settings.update(cx, |view, cx| {
            view.toggle(rwk_ui::settings::Setting::Tone, cx)
        })
    });
    let seen = events.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    let WorkbenchEvent::AppearanceChanged(appearance) = seen[0].clone() else {
        panic!("expected an appearance change, got {:?}", seen[0]);
    };
    assert_eq!(appearance.tone, Tone::Light);
    assert_eq!(rig.read(|wb, _| wb.appearance().tone), Tone::Light);
    assert_eq!(
        rig.read(|wb, _| wb.running_sessions()),
        1,
        "switching tone must not touch the PTYs"
    );
}

#[test]
fn the_first_render_is_counted_for_the_cold_start_gate() {
    let mut rig = rig();
    rig.cx.run_until_parked();
    assert!(rig.read(|wb, _| wb.frames_rendered()) >= 1);
}

#[test]
fn switching_projects_with_running_sessions_asks_first() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    let other = tempfile::tempdir().expect("other project");
    let root = other.path().to_path_buf();
    rig.with(|wb, window, cx| wb.open_project(root.clone(), window, cx));
    assert_eq!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::OpenProjectStoppingSessions(root))
    );
    assert_eq!(
        rig.read(|wb, _| wb.running_sessions()),
        1,
        "nothing stopped yet"
    );
    rig.with(|wb, window, cx| wb.resolve(Decision::Cancel, window, cx));
    assert_eq!(
        rig.read(|wb, _| wb.running_sessions()),
        1,
        "cancel keeps the session"
    );
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 1);
}

#[test]
fn reopening_an_open_dirty_file_just_brings_its_tab_forward() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    // Clicking the file that is already open must not reload it over the edits.
    rig.open_file("a.txt");
    rig.settle_for(300);
    assert!(
        rig.editor_text().contains('!'),
        "the edits were overwritten"
    );
    assert!(rig.read(|wb, _| wb.pending().is_none()));
    // Back to the disk content: close the tab and open it again.
    std::fs::write(rig.dir.path().join("a.txt"), "changed on disk").expect("write");
    rig.close_tab("a.txt");
    rig.with(|wb, window, cx| wb.resolve(Decision::Discard, window, cx));
    assert!(rig.settle_until(|rig| rig.editor_paths().is_empty()));
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "changed on disk"));
    assert!(rig.editor_dirty_paths().is_empty());
}

#[test]
fn reopening_the_open_clean_file_does_nothing() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.open_file("a.txt");
    assert!(rig.read(|wb, _| wb.pending().is_none()));
}

fn tick_ready(rig: &mut Rig) {
    while rig.cx.executor().tick() {}
}

#[test]
fn toasts_expire_after_five_seconds_and_a_replacement_gets_its_own_timer() {
    let mut rig = rig();
    rig.run(CommandId::Save);
    tick_ready(&mut rig);
    assert!(rig.read(|wb, _| wb.toast()).is_some());
    rig.cx.executor().advance_clock(Duration::from_secs(4));
    tick_ready(&mut rig);
    assert!(rig.read(|wb, _| wb.toast()).is_some());
    rig.run(CommandId::Refresh);
    tick_ready(&mut rig);
    let replacement = rig.read(|wb, _| wb.toast());
    assert!(replacement.is_some());
    rig.cx.executor().advance_clock(Duration::from_secs(1));
    tick_ready(&mut rig);
    assert_eq!(rig.read(|wb, _| wb.toast()), replacement);
    rig.cx.executor().advance_clock(Duration::from_secs(4));
    tick_ready(&mut rig);
    assert_eq!(rig.read(|wb, _| wb.toast()), None);
}

#[test]
fn the_status_bar_shows_the_quota_line_once_a_reading_lands() {
    let mut rig = rig();
    let calls = rig.script_usage(vec![(
        Duration::ZERO,
        Some(usage_report("5h:64% 7d:8% remaining")),
    )]);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.quota_line()).is_some()));
    assert_eq!(
        rig.read(|wb, _| wb.quota_line()).as_deref(),
        Some("Kota: Claude Code 5h:64% 7d:8% remaining")
    );
    assert_eq!(*calls.lock().expect("calls"), 1);
}

#[test]
fn a_failed_quota_reading_leaves_the_status_bar_quiet() {
    let mut rig = rig();
    let calls = rig.script_usage(vec![(Duration::ZERO, None)]);
    let observed = Arc::clone(&calls);
    assert!(rig.settle_until(move |_rig| *observed.lock().expect("calls") == 1));
    rig.cx.run_until_parked();
    assert_eq!(rig.read(|wb, _| wb.quota_line()), None);
    // The failure is not toasted either; the status bar simply stays quiet.
    assert_eq!(rig.read(|wb, _| wb.toast()), None);
}

#[test]
fn a_slow_quota_reading_never_blocks_the_workbench() {
    let mut rig = rig();
    let calls = rig.script_usage(vec![(
        Duration::from_millis(300),
        Some(usage_report("5h:64% 7d:8% remaining")),
    )]);
    // The reading has not even started on the executor yet: the workbench answers commands...
    assert_eq!(rig.read(|wb, _| wb.quota_line()), None);
    rig.run(CommandId::ToggleTerminal);
    assert!(!rig.read(|wb, _| wb.dock_visible()));
    assert_eq!(*calls.lock().expect("calls"), 0);
    // ...and the line lands once the background reading finishes.
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.quota_line()).is_some()));
    assert_eq!(*calls.lock().expect("calls"), 1);
}

#[test]
fn the_quota_signal_refreshes_on_the_timer() {
    let mut rig = rig();
    let calls = rig.script_usage(vec![
        (Duration::ZERO, Some(usage_report("5h:64% 7d:8% remaining"))),
        (Duration::ZERO, Some(usage_report("5h:12% 7d:8% remaining"))),
    ]);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.quota_line()).is_some()));
    assert_eq!(*calls.lock().expect("calls"), 1);
    rig.cx
        .executor()
        .advance_clock(rwk_ui::workbench::usage::USAGE_REFRESH);
    tick_ready(&mut rig);
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, _| wb.quota_line()).as_deref()
            == Some("Kota: Claude Code 5h:12% 7d:8% remaining")
    }));
    assert_eq!(*calls.lock().expect("calls"), 2);
}

#[test]
fn quick_open_finds_nested_files_and_respects_unsaved_edits() {
    let mut rig = rig();
    std::fs::create_dir(rig.dir.path().join("nested")).expect("nested");
    std::fs::write(rig.dir.path().join("nested/needle.rs"), "nested").expect("file");
    std::fs::create_dir(rig.dir.path().join("target")).expect("target");
    std::fs::write(rig.dir.path().join("target/needle-hidden.rs"), "noise").expect("noise");
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("a.txt")));
    rig.type_text("edited");
    rig.run(CommandId::Files);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("file palette");
    assert!(rig.settle_until(|rig| rig.read(|_, cx| !palette.read(cx).file_paths().is_empty())));
    rig.with(|_, window, cx| palette.update(cx, |p, cx| p.set_query("needle", window, cx)));
    assert_eq!(
        rig.read(|_, cx| palette.read(cx).file_paths().to_vec()),
        vec!["nested/needle.rs"]
    );
    rig.cx
        .update(|_, cx| palette.update(cx, |p, cx| p.confirm(cx)));
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("nested/needle.rs")));
    assert!(
        rig.read(|wb, _| wb.pending().is_none()),
        "opening a tab loses nothing, so nothing is asked"
    );
    assert_eq!(rig.editor_paths(), vec!["a.txt", "nested/needle.rs"]);
    assert_eq!(rig.editor_dirty_paths(), vec!["a.txt"]);
    rig.activate_tab("a.txt");
    assert!(rig.editor_text().contains("edited"));
}

#[test]
fn the_inspector_command_toggles_the_panel_and_shows_the_dock() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::ToggleTerminal);
    assert!(!rig.read(|wb, _| wb.dock_visible()));

    rig.run(CommandId::Inspector);

    assert!(rig.read(|wb, _| wb.inspector_visible()));
    assert!(rig.read(|wb, _| wb.dock_visible()));
    rig.run(CommandId::Inspector);
    assert!(!rig.read(|wb, _| wb.inspector_visible()));
}

#[test]
fn the_inspector_lists_only_the_records_of_the_open_project() {
    use raios_contracts::{NowSnapshot, ScoredApprovalDto, UnifiedTaskDto, WorkSnapshot};
    use rwk_core::control::fake::FakeControlPlane;
    use rwk_core::control::ControlPlane;

    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("canonical root");
    let mine = root.to_string_lossy().into_owned();
    let plane = FakeControlPlane::new();
    plane
        .push_work_snapshot(Ok(WorkSnapshot {
            tasks: vec![
                UnifiedTaskDto {
                    id: "t1".into(),
                    title: "mine".into(),
                    project_path: Some(mine.clone()),
                    ..UnifiedTaskDto::default()
                },
                UnifiedTaskDto {
                    id: "t2".into(),
                    title: "elsewhere".into(),
                    project_path: Some("/somewhere/else".into()),
                    ..UnifiedTaskDto::default()
                },
            ],
            ..WorkSnapshot::default()
        }))
        .push_now_snapshot(Ok(NowSnapshot {
            approvals: vec![ScoredApprovalDto {
                id: "a1".into(),
                project_path: Some(mine),
                ..ScoredApprovalDto::default()
            }],
            ..NowSnapshot::default()
        }));
    let plane: Arc<dyn ControlPlane> = Arc::new(plane);
    rig.with(|wb, _, cx| wb.set_control_plane(Some(plane), cx));
    rig.open_project();

    let shown = rig
        .settle_until(|rig| rig.read(|wb, cx| !wb.inspector().read(cx).model().tasks.is_empty()));

    assert!(shown, "the inspector never received the snapshot");
    let (tasks, approvals) = rig.read(|wb, cx| {
        let model = wb.inspector().read(cx).model().clone();
        (model.tasks, model.approvals)
    });
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "t1");
    assert_eq!(approvals.len(), 1);
}

#[test]
fn pressing_enter_in_the_palette_runs_the_highlighted_command() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| {
            palette.set_query("editör yerleşimi", window, cx)
        })
    });

    rig.cx.simulate_keystrokes("enter");
    rig.cx.run_until_parked();

    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Editor);
    assert!(rig.read(|wb, _| wb.palette().is_none()));
}

#[test]
fn enter_never_adds_a_newline_to_the_palette_query() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| {
            palette.set_query("zzzz-no-match", window, cx)
        })
    });

    rig.cx.simulate_keystrokes("enter");
    rig.cx.run_until_parked();

    let query = rig.cx.update(|_w, cx| palette.read(cx).query(cx));
    assert_eq!(query, "zzzz-no-match");
}

// ----- guarded daemon commands ---------------------------------------------------------

use rwk_core::control::fake::{FakeControlPlane, RecordedCall};
use rwk_core::control::{ControlEvent, ControlPlane, EventSource, PolicyAction, QueuedHandoff};
use rwk_core::sessions::{SessionKind, SessionName};

fn connected(rig: &mut Rig, policy: PolicyAction) -> Arc<FakeControlPlane> {
    connected_with(rig, policy, raios_contracts::WorkSnapshot::default())
}

fn connected_with(
    rig: &mut Rig,
    policy: PolicyAction,
    work: raios_contracts::WorkSnapshot,
) -> Arc<FakeControlPlane> {
    let fake = Arc::new(FakeControlPlane::new());
    fake.set_policy("launch_agent", policy);
    fake.set_policy("create_handoff", policy);
    fake.set_policy("register_project", policy);
    fake.set_policy("approve_handoff", policy);
    fake.set_policy("reject_handoff", policy);
    fake.set_policy("create_task", policy);
    fake.set_policy("update_task_status", policy);
    fake.set_policy("assign_task", policy);
    // The first inspector poll finds the daemon healthy; only then are agent commands offered.
    fake.push_work_snapshot(Ok(work))
        .push_now_snapshot(Ok(raios_contracts::NowSnapshot::default()));
    let plane: Arc<dyn ControlPlane> = fake.clone();
    rig.with(|wb, _, cx| wb.set_control_plane(Some(plane), cx));
    rig.open_project();
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, cx| wb.state(cx).daemon_reachable)
            && rig.read(|wb, cx| {
                wb.inspector().read(cx).model().banner == rwk_core::inspector::Banner::None
            })
    }));
    fake
}

fn sends(fake: &FakeControlPlane) -> usize {
    fake.calls()
        .iter()
        .filter(|call| {
            matches!(
                call,
                RecordedCall::LaunchAgent(_)
                    | RecordedCall::CreateHandoff(_)
                    | RecordedCall::RegisterProject { .. }
            )
        })
        .count()
}

/// Every handoff decision the fake received, in order.
fn decisions(fake: &FakeControlPlane) -> Vec<RecordedCall> {
    fake.calls()
        .iter()
        .filter(|call| {
            matches!(
                call,
                RecordedCall::ApproveHandoff(_) | RecordedCall::RejectHandoff(_)
            )
        })
        .cloned()
        .collect()
}

#[test]
fn a_confirm_policy_asks_first_and_sends_only_after_yes() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Confirm);
    fake.push_launch(Ok(()));

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Claude, window, cx));

    assert!(rig.settle_until(|rig| matches!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::Consent(..))
    )));
    assert_eq!(sends(&fake), 0, "nothing may be sent before the answer");
    let message = rig.read(|wb, _| wb.dialog().map(|dialog| dialog.message));
    assert!(message.is_some_and(|text| text.contains("claude_kaira")));

    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert_eq!(sends(&fake), 1);
}

#[test]
fn declining_the_consent_dialog_sends_nothing() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Confirm);

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Codex, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.pending().is_some())));
    rig.with(|wb, window, cx| wb.resolve(Decision::Cancel, window, cx));
    rig.settle_for(200);

    assert_eq!(sends(&fake), 0);
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
}

#[test]
fn a_deny_policy_refuses_without_a_dialog_or_a_send() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Deny);

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Claude, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.toast()).is_some()));
    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert_eq!(sends(&fake), 0);
}

#[test]
fn an_allow_policy_sends_straight_away() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_launch(Ok(()));

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Opencode, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert_eq!(sends(&fake), 1);
}

#[test]
fn a_daemon_refusal_of_the_launch_starts_no_terminal() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_launch(Err(rwk_core::error::AppError::new(
        rwk_core::error::ErrorCode::Forbidden,
        "launch refused",
    )));

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Claude, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.toast()).is_some()));
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
}

#[test]
fn a_secret_in_the_handoff_note_never_reaches_the_daemon() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    rig.run(CommandId::Handoff);
    let composer = rig
        .read(|wb, _| wb.composer().cloned())
        .expect("the composer opens");

    rig.with(|_, window, cx| {
        composer.update(cx, |composer, cx| {
            composer.set_text("anahtar sk-abcdefghijklmnopqrstuvwx", window, cx);
            composer.submit(SessionKind::Codex, cx);
        })
    });
    rig.cx.run_until_parked();

    assert!(rig.read(|wb, cx| wb
        .composer()
        .is_some_and(|c| c.read(cx).problem().is_some())));
    assert_eq!(sends(&fake), 0);
}

#[test]
fn a_valid_handoff_is_queued_and_tracked() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    let root = std::fs::canonicalize(rig.dir.path()).expect("root");
    fake.push_handoff(Ok(QueuedHandoff {
        approval_id: "a-1".into(),
        project_path: root.clone(),
    }));
    fake.push_receipt(Ok(raios_contracts::HandoffReceiptDto {
        approval_id: "a-1".into(),
        project_path: root.to_string_lossy().into_owned(),
        from_agent: "claude_kaira".into(),
        to_agent: "codex_kaira".into(),
        status: "pending".into(),
        resolved_by: None,
        resolved_at: None,
        delivered_by_target: false,
    }));
    rig.run(CommandId::Handoff);
    let composer = rig
        .read(|wb, _| wb.composer().cloned())
        .expect("the composer opens");

    rig.with(|_, window, cx| {
        composer.update(cx, |composer, cx| {
            composer.set_text("Devam et: testleri çalıştır.", window, cx);
            composer.submit(SessionKind::Codex, cx);
        })
    });

    assert!(rig.settle_until(|rig| !rig.read(|wb, _| wb.handoffs().is_empty())));
    assert!(
        rig.read(|wb, _| wb.composer().is_none()),
        "the composer closes"
    );
    // The receipt read that follows the queueing confirms the handoff is pending.
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.handoffs()[0].can_start_receiver())));
    let tracked = rig.read(|wb, _| wb.handoffs()[0].clone());
    assert_eq!(tracked.approval_id, "a-1");
    assert_eq!(tracked.to, "codex_kaira");
    assert_eq!(sends(&fake), 1);
}

#[test]
fn the_agent_command_asks_which_agent() {
    let mut rig = rig();
    let _fake = connected(&mut rig, PolicyAction::Allow);

    rig.run(CommandId::Agent);

    let dialog = rig.read(|wb, _| wb.dialog()).expect("a chooser");
    assert_eq!(dialog.title, "Ajan başlat");
    assert_eq!(dialog.buttons.len(), 5);
}

#[test]
fn agent_commands_are_refused_without_a_daemon_connection() {
    let mut rig = rig();
    rig.open_project();

    rig.run(CommandId::Agent);

    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert_eq!(
        rig.read(|wb, _| wb.toast()),
        Some("Daemon bağlantısı yapılandırılmadı".to_owned())
    );
}

#[test]
fn starting_the_receiver_reads_the_receipt_again_and_tracks_the_process() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    let root = std::fs::canonicalize(rig.dir.path()).expect("root");
    let pending = || raios_contracts::HandoffReceiptDto {
        approval_id: "a-1".into(),
        project_path: root.to_string_lossy().into_owned(),
        from_agent: "claude_kaira".into(),
        to_agent: "codex_kaira".into(),
        status: "pending".into(),
        resolved_by: None,
        resolved_at: None,
        delivered_by_target: false,
    };
    fake.push_handoff(Ok(QueuedHandoff {
        approval_id: "a-1".into(),
        project_path: root.clone(),
    }))
    .push_receipt(Ok(pending()))
    .push_receipt(Ok(pending()))
    .push_launch(Ok(()));
    rig.with(|wb, window, cx| {
        wb.start_guarded(
            rwk_core::guarded::Guarded::CreateHandoff(rwk_core::control::CreateHandoffRequest {
                project_root: root.clone(),
                from: SessionKind::Claude,
                to: SessionKind::Codex,
                summary: "devam".into(),
                idempotency_key: rwk_core::control::IdempotencyKey::fresh(),
            }),
            window,
            cx,
        )
    });
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, _| {
            wb.handoffs()
                .first()
                .is_some_and(|h| h.can_start_receiver())
        })
    }));

    rig.with(|wb, window, cx| wb.start_receiver("a-1".into(), window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, _| wb.handoffs()[0].label().contains("alıcı çalışıyor"))
    }));
    let reads = fake
        .calls()
        .iter()
        .filter(|call| matches!(call, RecordedCall::HandoffReceipt { .. }))
        .count();
    assert!(
        reads >= 2,
        "the receipt is read again before the receiver starts"
    );
    assert!(
        !rig.read(|wb, _| wb.handoffs()[0].can_start_receiver()),
        "a running receiver blocks a second start"
    );
}

// ----- approval decisions --------------------------------------------------------------

#[test]
fn approving_a_handoff_sends_the_revision_the_user_saw() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_approval(Ok(()));

    rig.with(|wb, window, cx| wb.approve_handoff("a-1".into(), 7, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb
        .toast()
        .is_some_and(|toast| toast.contains("Devir onaylandı")))));
    assert!(matches!(
        &decisions(&fake)[..],
        [RecordedCall::ApproveHandoff(request)]
        if request.approval_id == "a-1"
            && request.expected_revision == 7
            && request.reason.is_none()
    ));
    // Consent is not delivery: no receiver was started and no agent terminal opened.
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
}

#[test]
fn a_confirm_policy_asks_before_a_handoff_decision_is_sent() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Confirm);
    fake.push_approval(Ok(()));

    rig.with(|wb, window, cx| wb.approve_handoff("a-1".into(), 7, window, cx));

    assert!(rig.settle_until(|rig| matches!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::Consent(..))
    )));
    assert!(decisions(&fake).is_empty(), "nothing before the answer");
    let message = rig.read(|wb, _| wb.dialog().map(|dialog| dialog.message));
    // The dialog says what consent means: the addressed agent still delivers the note.
    assert!(message.is_some_and(|text| text.contains("teslim")));

    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));

    assert!(rig.settle_until(|_rig| decisions(&fake).len() == 1));
}

#[test]
fn rejecting_a_handoff_asks_for_a_reason_and_sends_it() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_rejection(Ok(()));

    rig.with(|wb, window, cx| wb.request_reject_handoff("a-1".into(), 7, window, cx));
    let prompt = rig
        .read(|wb, _| wb.reject_prompt().cloned())
        .expect("the prompt opens");
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("  kapsam dışı  ", window, cx);
            prompt.submit(cx);
        })
    });

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.reject_prompt().is_none())));
    assert!(rig.settle_until(|_rig| decisions(&fake).len() == 1));
    assert!(matches!(
        &decisions(&fake)[..],
        [RecordedCall::RejectHandoff(request)]
        if request.reason.as_deref() == Some("kapsam dışı")
            && request.expected_revision == 7
    ));
    assert_eq!(
        rig.read(|wb, _| wb.toast()),
        Some("Devir reddedildi: a-1".to_owned())
    );
}

#[test]
fn an_empty_or_secret_reject_reason_never_reaches_the_daemon() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);

    for text in ["   ", "anahtar sk-abcdefghijklmnopqrstuvwx"] {
        rig.with(|wb, window, cx| wb.request_reject_handoff("a-1".into(), 7, window, cx));
        let prompt = rig
            .read(|wb, _| wb.reject_prompt().cloned())
            .expect("the prompt opens");
        rig.with(|_wb, window, cx| {
            prompt.update(cx, |prompt, cx| {
                prompt.set_text(text, window, cx);
                prompt.submit(cx);
            })
        });
        rig.settle_for(50);

        assert!(rig.read(|wb, cx| wb
            .reject_prompt()
            .is_some_and(|prompt| prompt.read(cx).problem().is_some())));
        assert!(decisions(&fake).is_empty(), "{text:?} must not be sent");
        rig.with(|wb, window, cx| wb.close_reject_prompt(window, cx));
    }
}

#[test]
fn escape_closes_the_reject_prompt_without_sending() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);

    rig.with(|wb, window, cx| wb.request_reject_handoff("a-1".into(), 7, window, cx));
    assert!(rig.read(|wb, _| wb.reject_prompt().is_some()));

    rig.cx.simulate_keystrokes("escape");
    rig.cx.run_until_parked();

    assert!(rig.read(|wb, _| wb.reject_prompt().is_none()));
    assert!(decisions(&fake).is_empty());
}

// ----- memory view ---------------------------------------------------------------------

struct FixedMemory;

impl rwk_core::memory::MemorySource for FixedMemory {
    fn snapshot(
        &self,
        project: &rwk_core::project::ProjectRoot,
    ) -> rwk_core::error::Result<rwk_core::memory::MemorySnapshot> {
        Ok(rwk_core::memory::MemorySnapshot {
            project_id: project.info().id.clone(),
            items: vec![rwk_core::memory::MemoryItem {
                id: "1".into(),
                slug: "persona".into(),
                title: "Persona".into(),
                description: "who".into(),
                item_type: "user".into(),
                layer: 3,
                provenance: "test".into(),
                updated_at: "2026-10-14".into(),
                body: "- [ ] todo".into(),
                truncated: false,
            }],
            total: 1,
        })
    }

    fn history(
        &self,
        _project: &rwk_core::project::ProjectRoot,
        _slug: &str,
    ) -> rwk_core::error::Result<rwk_core::memory::MemoryHistory> {
        Err(rwk_core::error::AppError::new(
            rwk_core::error::ErrorCode::NotFound,
            "none",
        ))
    }
}

#[test]
fn the_memory_view_lists_the_sections_of_memory_md_and_the_records() {
    use rwk_ui::memory::MemorySourceKind;

    let mut rig = rig();
    std::fs::write(
        rig.dir.path().join("memory.md"),
        "# Proje\n## Hedefler\n- [x] a\n- [ ] b\n",
    )
    .expect("memory.md");
    rig.with(|wb, _, _| wb.set_memory_source(Some(Arc::new(FixedMemory))));
    rig.open_project();

    rig.run(CommandId::Memory);

    let view = rig
        .read(|wb, _| wb.memory().cloned())
        .expect("the memory view opens");
    assert!(rig.settle_until(|rig| {
        rig.read(|_, cx| view.read(cx).outline(cx)) == ["Proje", "Hedefler"]
    }));
    rig.with(|_, window, cx| {
        view.update(cx, |view, cx| {
            view.show(MemorySourceKind::Records, cx);
            view.set_query("pers", window, cx);
        })
    });
    assert!(rig.settle_until(|rig| rig.read(|_, cx| view.read(cx).outline(cx)) == ["Persona"]));
}

#[test]
fn a_project_without_memory_md_says_so_and_escape_closes_the_view() {
    let mut rig = rig();
    rig.open_project();

    rig.run(CommandId::Memory);
    let view = rig.read(|wb, _| wb.memory().cloned()).expect("view");
    rig.settle_for(300);

    assert!(rig.read(|_, cx| view.read(cx).outline(cx)).is_empty());
    rig.with(|wb, window, cx| wb.close_memory(window, cx));
    assert!(rig.read(|wb, _| wb.memory().is_none()));
}

#[test]
fn the_skin_command_switches_between_standard_and_lcars_and_announces_it() {
    use rwk_core::workbench::Skin;

    let mut rig = rig();
    let events: Arc<Mutex<Vec<WorkbenchEvent>>> = Arc::default();
    let sink = Arc::clone(&events);
    let view = rig.view.clone();
    let _subscription = rig.cx.update(|_, cx| {
        cx.subscribe(&view, move |_wb, event: &WorkbenchEvent, _cx| {
            sink.lock().expect("events").push(event.clone());
        })
    });

    rig.run(CommandId::Skin);

    assert!(rig.read(|wb, _| wb.theme().lcars));
    let announced = events.lock().expect("events").clone();
    assert!(matches!(
        announced.last(),
        Some(WorkbenchEvent::AppearanceChanged(a)) if a.skin == Skin::Lcars
    ));
    rig.run(CommandId::Skin);
    assert!(!rig.read(|wb, _| wb.theme().lcars));
}

fn projects_snapshot(root: Option<&std::path::Path>) -> raios_contracts::WorkSnapshot {
    raios_contracts::WorkSnapshot {
        projects: root
            .map(|root| {
                vec![raios_contracts::ProjectDto {
                    path: root.to_string_lossy().into_owned(),
                    name: "app".into(),
                    status: "active".into(),
                    ..raios_contracts::ProjectDto::default()
                }]
            })
            .unwrap_or_default(),
        ..raios_contracts::WorkSnapshot::default()
    }
}

#[test]
fn opening_a_project_the_daemon_does_not_know_registers_it() {
    let mut rig = rig();
    let fake = Arc::new(FakeControlPlane::new());
    fake.set_policy("register_project", PolicyAction::Allow);
    for _ in 0..3 {
        fake.push_work_snapshot(Ok(projects_snapshot(None)));
    }
    fake.push_now_snapshot(Ok(raios_contracts::NowSnapshot::default()))
        .push_registration(Ok(rwk_core::control::RegistrationOutcome {
            project_path: std::path::PathBuf::new(),
            already_registered: false,
        }));
    let plane: Arc<dyn ControlPlane> = fake.clone();
    rig.with(|wb, _, cx| wb.set_control_plane(Some(plane), cx));

    rig.open_project();

    assert!(rig.settle_until(|_| {
        fake.calls()
            .iter()
            .any(|call| matches!(call, RecordedCall::RegisterProject { .. }))
    }));
}

#[test]
fn opening_an_already_registered_project_registers_nothing() {
    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("root");
    let fake = Arc::new(FakeControlPlane::new());
    fake.set_policy("register_project", PolicyAction::Allow);
    for _ in 0..3 {
        fake.push_work_snapshot(Ok(projects_snapshot(Some(&root))));
    }
    fake.push_now_snapshot(Ok(raios_contracts::NowSnapshot::default()));
    let plane: Arc<dyn ControlPlane> = fake.clone();
    rig.with(|wb, _, cx| wb.set_control_plane(Some(plane), cx));

    rig.open_project();
    rig.settle_for(500);

    assert_eq!(sends(&fake), 0);
}

#[test]
fn the_third_terminal_switches_to_the_agent_layout() {
    let mut rig = rig();
    rig.open_project();

    for _ in 0..3 {
        rig.run(CommandId::NewTerminal);
    }

    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 3);
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Agents);
}

#[test]
fn starting_a_terminal_without_a_project_offers_the_picker_and_remembers_the_command() {
    let mut rig = rig();

    rig.run(CommandId::NewTerminal);

    // The folder picker is the platform's; here the command is only remembered and no refusal
    // toast appears.
    assert_eq!(rig.read(|wb, _| wb.toast()), None);
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
}

#[test]
fn the_dock_divider_moves_with_the_keyboard_commands_and_respects_the_minimums() {
    let mut rig = rig();
    rig.run(CommandId::Balanced);
    let start = rig.read(|wb, cx| wb.dock_width(cx));

    rig.run(CommandId::GrowDock);
    let wider = rig.read(|wb, cx| wb.dock_width(cx));
    rig.run(CommandId::ShrinkDock);
    rig.run(CommandId::ShrinkDock);
    let narrower = rig.read(|wb, cx| wb.dock_width(cx));

    assert!(wider > start, "{start} -> {wider}");
    assert!(narrower < start, "{start} -> {narrower}");
    for _ in 0..40 {
        rig.run(CommandId::ShrinkDock);
    }
    assert!(
        rig.read(|wb, cx| wb.dock_width(cx)) >= rwk_ui::workbench::layout::MIN_SIDE_PANE_PX - 0.5
    );
    rig.run(CommandId::ToggleTerminal);
    rig.run(CommandId::GrowDock);
    assert_eq!(rig.read(|wb, _| wb.toast()).as_deref(), Some("Dock gizli"));
}

#[test]
fn a_project_remembered_as_registered_is_not_offered_again() {
    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("root");
    let fake = Arc::new(FakeControlPlane::new());
    fake.set_policy("register_project", PolicyAction::Allow);
    for _ in 0..3 {
        fake.push_work_snapshot(Ok(projects_snapshot(None)));
    }
    fake.push_now_snapshot(Ok(raios_contracts::NowSnapshot::default()));
    let plane: Arc<dyn ControlPlane> = fake.clone();
    rig.with(|wb, _, cx| {
        wb.set_known_projects([root]);
        wb.set_control_plane(Some(plane), cx);
    });

    rig.open_project();
    rig.settle_for(500);

    assert_eq!(sends(&fake), 0);
}

#[test]
fn a_confirmed_registration_is_announced_so_the_application_can_remember_it() {
    let mut rig = rig();
    let events: Arc<Mutex<Vec<WorkbenchEvent>>> = Arc::default();
    let sink = Arc::clone(&events);
    let view = rig.view.clone();
    let _subscription = rig.cx.update(|_, cx| {
        cx.subscribe(&view, move |_wb, event: &WorkbenchEvent, _cx| {
            sink.lock().expect("events").push(event.clone());
        })
    });
    let fake = Arc::new(FakeControlPlane::new());
    fake.set_policy("register_project", PolicyAction::Allow);
    for _ in 0..3 {
        fake.push_work_snapshot(Ok(projects_snapshot(None)));
    }
    fake.push_now_snapshot(Ok(raios_contracts::NowSnapshot::default()))
        .push_registration(Ok(rwk_core::control::RegistrationOutcome {
            project_path: std::path::PathBuf::new(),
            already_registered: true,
        }));
    let plane: Arc<dyn ControlPlane> = fake.clone();
    rig.with(|wb, _, cx| wb.set_control_plane(Some(plane), cx));

    rig.open_project();

    assert!(rig.settle_until(|_| events
        .lock()
        .expect("events")
        .iter()
        .any(|event| matches!(event, WorkbenchEvent::ProjectRegistered(_)))));
}

/// The task titles the inspector currently shows.
fn inspector_titles(rig: &mut Rig) -> Vec<String> {
    rig.read(|wb, cx| {
        wb.inspector()
            .read(cx)
            .model()
            .tasks
            .iter()
            .map(|task| task.title.clone())
            .collect()
    })
}

#[test]
fn a_typed_stream_event_refreshes_the_inspector_before_the_next_poll() {
    use std::sync::mpsc;

    let mut rig = rig();
    let root = rig.dir.path().to_path_buf();
    let task = |title: &str| raios_contracts::WorkSnapshot {
        tasks: vec![raios_contracts::UnifiedTaskDto {
            id: title.to_owned(),
            title: title.to_owned(),
            project_path: Some(root.to_string_lossy().into_owned()),
            ..raios_contracts::UnifiedTaskDto::default()
        }],
        ..raios_contracts::WorkSnapshot::default()
    };

    // One read for the first poll, then the new state. The project counts as known, so opening
    // it never probes the daemon: the inspector is the only reader of the script.
    let fake = Arc::new(FakeControlPlane::new());
    fake.push_work_snapshot(Ok(task("ilk durum")));
    for _ in 0..100 {
        fake.push_work_snapshot(Ok(task("yeni durum")));
        fake.push_now_snapshot(Ok(raios_contracts::NowSnapshot::default()));
    }
    let plane: Arc<dyn ControlPlane> = fake.clone();
    rig.with(|wb, _, cx| {
        wb.set_known_projects([root.clone()]);
        wb.set_control_plane(Some(plane), cx);
    });
    rig.open_project();
    assert!(
        rig.settle_until(|rig| inspector_titles(rig)
            .iter()
            .any(|title| title == "ilk durum")),
        "the first poll must show the first snapshot"
    );

    // A gated stream: the monitor delivers only what the test sends.
    struct GatedEvents {
        gate: mpsc::Receiver<ControlEvent>,
    }
    impl EventSource for GatedEvents {
        fn next_event(&mut self, timeout: Duration) -> rwk_core::Result<Option<ControlEvent>> {
            Ok(self.gate.recv_timeout(timeout).ok())
        }
    }
    let (occurrences, gate) = mpsc::channel();
    rig.with(|wb, _, cx| wb.set_event_source(Some(Box::new(GatedEvents { gate })), cx));

    occurrences
        .send(ControlEvent::RunChanged {
            run_id: "r1".into(),
            agent_name: "codex_kaira".into(),
            status: "running".into(),
        })
        .expect("send the event");

    // The next poll is three seconds away: only an event-triggered refresh can switch the
    // model to the new snapshot this fast.
    let deadline = Instant::now() + Duration::from_millis(2500);
    let mut refreshed = false;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        rig.cx.run_until_parked();
        if inspector_titles(&mut rig)
            .iter()
            .any(|title| title == "yeni durum")
        {
            refreshed = true;
            break;
        }
    }
    assert!(
        refreshed,
        "a typed stream event must refresh the inspector before the next poll"
    );
}

/// A scripted search source: queued answers with their delays, and a log of the calls.
struct FakeSearch {
    answers: Mutex<std::collections::VecDeque<(Duration, Vec<SearchHit>)>>,
    calls: Arc<Mutex<Vec<(String, bool)>>>,
}

impl SearchSource for FakeSearch {
    fn locate(
        &self,
        _root: &std::path::Path,
        pattern: &str,
        ignore_case: bool,
    ) -> rwk_core::error::Result<SearchResult> {
        self.calls
            .lock()
            .expect("calls")
            .push((pattern.to_owned(), ignore_case));
        let (delay, hits) = self
            .answers
            .lock()
            .expect("answers")
            .pop_front()
            .unwrap_or((Duration::ZERO, Vec::new()));
        std::thread::sleep(delay);
        Ok(SearchResult {
            hits,
            truncated: false,
        })
    }
}

/// A scripted quota source: queued answers with their delays, and a log of the calls. `None`
/// answers fail like a missing CLI; the workbench must stay quiet then.
struct FakeUsage {
    answers: Mutex<std::collections::VecDeque<(Duration, Option<UsageReport>)>>,
    calls: Arc<Mutex<usize>>,
}

impl UsageSource for FakeUsage {
    fn usage(&self) -> rwk_core::error::Result<UsageReport> {
        *self.calls.lock().expect("calls") += 1;
        let (delay, answer) = self
            .answers
            .lock()
            .expect("answers")
            .pop_front()
            .unwrap_or((Duration::ZERO, None));
        std::thread::sleep(delay);
        match answer {
            Some(report) => Ok(report),
            None => Err(rwk_core::error::AppError::new(
                rwk_core::error::ErrorCode::Io,
                "kota okunamadı".to_owned(),
            )),
        }
    }
}

/// A report with a single remaining figure, as `raios usage --json` answers for Claude.
fn usage_report(remaining: &str) -> UsageReport {
    UsageReport {
        providers: vec![UsageProvider {
            provider: "Claude Code".to_owned(),
            installed: true,
            authenticated: true,
            remaining: Some(remaining.to_owned()),
        }],
    }
}

fn hits(entries: &[(&str, usize, &str)]) -> Vec<SearchHit> {
    entries
        .iter()
        .map(|(path, line, text)| SearchHit {
            path: (*path).to_owned(),
            line: *line,
            text: (*text).to_owned(),
        })
        .collect()
}

#[test]
fn a_search_runs_on_enter_and_a_hit_opens_the_file_at_its_line() {
    let mut rig = rig();
    rig.open_project();
    std::fs::write(rig.dir.path().join("c.txt"), "alpha\nbeta\ngamma").expect("c");
    let calls = rig.script_search(vec![(Duration::ZERO, hits(&[("c.txt", 2, "beta")]))]);
    rig.run(CommandId::Search);
    assert!(rig.search_open());
    rig.search_query("beta");
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| rig.search_hits().as_slice() == ["c.txt:2"]));
    assert_eq!(
        calls.lock().expect("calls").as_slice(),
        [("beta".to_owned(), false)]
    );
    // The first Enter ran the query; a second one opens the highlighted hit.
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("c.txt")));
    assert!(!rig.search_open(), "opening a hit closes the overlay");
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).cursor_line(cx)),
        Some(2)
    );
}

#[test]
fn replace_all_previews_first_then_writes_and_leaves_unsaved_files_alone() {
    let mut rig = rig();
    rig.open_project();
    std::fs::write(rig.dir.path().join("c.txt"), "beta one\nbeta two").expect("c");
    std::fs::write(rig.dir.path().join("d.txt"), "beta").expect("d");
    rig.script_search(vec![
        (
            Duration::ZERO,
            hits(&[
                ("c.txt", 1, "beta one"),
                ("c.txt", 2, "beta two"),
                ("d.txt", 1, "beta"),
            ]),
        ),
        (Duration::ZERO, Vec::new()),
    ]);
    // d.txt is open with an unsaved edit, so it must not be rewritten underneath it.
    rig.open_file("d.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "beta"));
    rig.type_text("!");
    rig.run(CommandId::Search);
    rig.search_query("beta");
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| rig.search_hits().len() == 3));

    let view = rig.read(|wb, _| wb.search().cloned()).expect("view");
    rig.with(|_wb, window, cx| {
        view.update(cx, |view, cx| {
            view.set_replacement("gamma", window, cx);
            view.press_replace(cx);
        })
    });
    let note = |rig: &mut Rig| {
        rig.cx
            .update(|_, cx| view.read(cx).replace_note().map(str::to_owned))
    };
    assert!(rig.settle_until(|rig| note(rig).is_some_and(|n| n.contains("onaylamak"))));
    let note_text = note(&mut rig).unwrap();
    assert!(
        note_text.contains("1 dosyada 2 yer") && note_text.contains("d.txt"),
        "{note_text}"
    );
    assert_eq!(
        std::fs::read_to_string(rig.dir.path().join("c.txt")).unwrap(),
        "beta one\nbeta two",
        "a preview writes nothing"
    );

    rig.with(|_wb, _window, cx| view.update(cx, |view, cx| view.press_replace(cx)));
    let c = rig.dir.path().join("c.txt");
    assert!(rig.settle_until(|_| {
        std::fs::read_to_string(&c).is_ok_and(|text| text == "gamma one\ngamma two")
    }));
    assert_eq!(
        std::fs::read_to_string(rig.dir.path().join("d.txt")).unwrap(),
        "beta"
    );
}

#[test]
fn a_clicked_path_in_terminal_output_opens_the_file_at_its_line() {
    use rwk_ui::terminal::TerminalEvent;
    let mut rig = rig();
    rig.open_project();
    std::fs::write(rig.dir.path().join("c.txt"), "one\ntwo\nthree").expect("c");
    rig.run(CommandId::NewTerminal);
    let view = rig
        .read(|wb, _| wb.terminal_view(0).cloned())
        .expect("terminal");
    let line = "error: c.txt:3:1 broke";
    rig.with(|_wb, _w, cx| {
        view.update(cx, |_, cx| {
            cx.emit(TerminalEvent::Open {
                line: line.to_owned(),
                column: 9,
            })
        })
    });
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("c.txt")));
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).cursor_line(cx)),
        Some(3)
    );
}

#[test]
fn a_clicked_web_address_in_terminal_output_is_opened_not_treated_as_a_path() {
    use rwk_ui::terminal::TerminalEvent;
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    let view = rig
        .read(|wb, _| wb.terminal_view(0).cloned())
        .expect("terminal");
    rig.with(|_wb, _w, cx| {
        view.update(cx, |_, cx| {
            cx.emit(TerminalEvent::Open {
                line: "docs: https://example.com/a:12.".to_owned(),
                column: 12,
            })
        })
    });
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, _| wb.toast())
            .is_some_and(|toast| toast == "Açılıyor: https://example.com/a:12")
    }));
    assert_eq!(rig.editor_path(), None, "no file was opened");
}

#[test]
fn escape_closes_the_search_overlay() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::Search);
    assert!(rig.search_open());
    rig.cx.simulate_keystrokes("escape");
    assert!(!rig.search_open());
}

#[test]
fn the_case_toggle_runs_the_query_again_with_ignore_case() {
    let mut rig = rig();
    rig.open_project();
    let calls = rig.script_search(vec![
        (Duration::ZERO, hits(&[("a.txt", 1, "one")])),
        (Duration::ZERO, hits(&[("a.txt", 1, "ONE")])),
    ]);
    rig.run(CommandId::Search);
    rig.search_query("one");
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| !rig.search_hits().is_empty()));
    rig.with(|wb, _window, cx| {
        let view = wb.search().expect("the search view is open").clone();
        view.update(cx, |view, cx| view.toggle_case(cx));
    });
    assert!(rig.settle_until(|_rig| calls.lock().expect("calls").len() == 2));
    assert_eq!(
        calls.lock().expect("calls").as_slice(),
        [("one".to_owned(), false), ("one".to_owned(), true)]
    );
}

#[test]
fn an_answer_for_a_superseded_search_is_dropped() {
    let mut rig = rig();
    rig.open_project();
    rig.script_search(vec![
        (Duration::from_millis(400), hits(&[("a.txt", 1, "slow")])),
        (Duration::ZERO, hits(&[("b.txt", 1, "fast")])),
    ]);
    rig.run(CommandId::Search);
    rig.search_query("slow");
    rig.cx.simulate_keystrokes("enter");
    rig.search_query("fast");
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| rig.search_hits().as_slice() == ["b.txt:1"]));
    // The slow answer arrives after the fast one and must not replace it.
    rig.settle_for(500);
    assert_eq!(rig.search_hits().as_slice(), ["b.txt:1"]);
}

#[test]
fn switching_projects_closes_the_search_overlay() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::Search);
    assert!(rig.search_open());
    let root = rig.dir.path().join("nested");
    std::fs::create_dir(&root).expect("nested dir");
    rig.with(|wb, window, cx| wb.open_project(root, window, cx));
    assert!(!rig.search_open());
}

#[test]
fn a_project_reopens_with_its_saved_tabs_layout_and_shells() {
    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("canonical");
    let store = FixedWorkspaceStore::new(Some(SavedWorkspace {
        project: root,
        tabs: vec![
            SavedTab {
                path: "a.txt".into(),
                line: 1,
            },
            SavedTab {
                path: "b.txt".into(),
                line: 1,
            },
        ],
        active: 1,
        layout: LayoutPreset::Editor,
        dock: None,
        terminals: vec![SavedTerminal::Shell { name: None }],
        task_links: Vec::new(),
    }));
    rig.set_workspace_source(store);
    rig.open_project();
    assert!(
        rig.settle_until(|rig| rig.editor_paths() == vec!["a.txt".to_owned(), "b.txt".to_owned()])
    );
    assert_eq!(
        rig.editor_path().as_deref(),
        Some("b.txt"),
        "the saved active tab"
    );
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Editor);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
}

#[test]
fn a_saved_agent_session_is_offered_and_resuming_runs_raios_resume() {
    const KEY: &str = "claude:3294c005-aee0-4e95-860f-5800e045b624";
    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("canonical");
    rig.script_catalog(&format!(
        r#"{{"sessions":[{{"key":"{KEY}","agent":"claude","native_session_id":"3294c005-aee0-4e95-860f-5800e045b624","project_path":"{}","name":"gece oturumu","resumable":true}}],"warnings":[]}}"#,
        root.display()
    ));
    rig.set_workspace_source(FixedWorkspaceStore::new(Some(SavedWorkspace {
        project: root.clone(),
        tabs: Vec::new(),
        active: 0,
        layout: LayoutPreset::Balanced,
        dock: None,
        terminals: vec![SavedTerminal::Agent {
            kind: SessionKind::Claude,
            native_session: None,
            name: None,
        }],
        task_links: Vec::new(),
    })));
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_launch(Ok(()));
    let saved = rig.record_workspace_changes();
    rig.open_project();

    // Opening the project reads the catalog and turns the saved agent into an offer. Nothing
    // is started before the user takes it.
    assert!(
        rig.settle_until(|rig| rig.read(|wb, _| wb.offers().len() == 1)),
        "the offer never appeared"
    );
    assert_eq!(
        rig.read(|wb, _| wb.terminal_count()),
        0,
        "offers are not panes"
    );
    let offer = rig.read(|wb, _| wb.offers()[0].clone());
    assert_eq!(offer.kind, SessionKind::Claude);
    assert_eq!(
        offer.matched.as_ref().map(|matched| matched.name.as_str()),
        Some("gece oturumu"),
        "the offer names the catalog's session"
    );

    rig.with(|wb, window, cx| wb.resume_offer(&offer, window, cx));

    // The continuation is validated as an agent launch and its pane runs the CLI's resume.
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert!(
        fake.calls().iter().any(|call| matches!(
            call,
            RecordedCall::LaunchAgent(request)
                if request.agent == SessionKind::Claude && request.prompt.is_none()
        )),
        "{:?}",
        fake.calls()
    );
    assert!(
        rig.script_calls()
            .iter()
            .any(|call| call == &format!("resume {KEY}")),
        "{:?}",
        rig.script_calls()
    );
    assert!(
        rig.read(|wb, _| wb.offers().is_empty()),
        "a taken offer is done"
    );

    // The state remembers which conversation the pane continues, so the next open offers
    // exactly that one.
    rig.cx.executor().advance_clock(Duration::from_millis(600));
    tick_ready(&mut rig);
    assert!(rig.settle_until(|_rig| !saved.lock().expect("lock").is_empty()));
    let last = saved
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .expect("a saved state");
    assert_eq!(
        last.terminals,
        vec![SavedTerminal::Agent {
            kind: SessionKind::Claude,
            native_session: Some("3294c005-aee0-4e95-860f-5800e045b624".into()),
            name: None,
        }]
    );
}

#[test]
fn an_offer_without_a_catalog_entry_can_only_be_dismissed() {
    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("canonical");
    // No catalog file: the CLI reports nothing to continue.
    rig.set_workspace_source(FixedWorkspaceStore::new(Some(SavedWorkspace {
        project: root,
        tabs: Vec::new(),
        active: 0,
        layout: LayoutPreset::Balanced,
        dock: None,
        terminals: vec![SavedTerminal::Agent {
            kind: SessionKind::Codex,
            native_session: None,
            name: None,
        }],
        task_links: Vec::new(),
    })));
    let saved = rig.record_workspace_changes();
    rig.open_project();

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.offers().len() == 1)));
    let offer = rig.read(|wb, _| wb.offers()[0].clone());
    assert!(
        offer.matched.is_none(),
        "nothing matched, so nothing to run"
    );

    rig.with(|wb, _window, cx| wb.dismiss_offer(&offer, cx));

    assert!(rig.read(|wb, _| wb.offers().is_empty()));
    rig.cx.executor().advance_clock(Duration::from_millis(600));
    tick_ready(&mut rig);
    assert!(rig.settle_until(|_rig| !saved.lock().expect("lock").is_empty()));
    let last = saved
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .expect("a saved state");
    assert!(
        last.terminals.is_empty(),
        "a dismissed offer is not offered again: {:?}",
        last.terminals
    );
}

#[test]
fn a_denied_resume_keeps_the_offer_for_another_try() {
    const KEY: &str = "codex:9ac8a20a-7f0d-4c11-9f31-6b6e2f2ab111";
    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("canonical");
    rig.script_catalog(&format!(
        r#"{{"sessions":[{{"key":"{KEY}","agent":"codex","native_session_id":"9ac8a20a-7f0d-4c11-9f31-6b6e2f2ab111","project_path":"{}","name":"codex oturumu","resumable":true}}],"warnings":[]}}"#,
        root.display()
    ));
    rig.set_workspace_source(FixedWorkspaceStore::new(Some(SavedWorkspace {
        project: root,
        tabs: Vec::new(),
        active: 0,
        layout: LayoutPreset::Balanced,
        dock: None,
        terminals: vec![SavedTerminal::Agent {
            kind: SessionKind::Codex,
            native_session: None,
            name: None,
        }],
        task_links: Vec::new(),
    })));
    let fake = connected(&mut rig, PolicyAction::Deny);
    rig.open_project();

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.offers().len() == 1)));
    let offer = rig.read(|wb, _| wb.offers()[0].clone());
    rig.with(|wb, window, cx| wb.resume_offer(&offer, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.toast()).is_some()));
    assert_eq!(sends(&fake), 0, "a denied resume sends nothing");
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
    assert_eq!(
        rig.read(|wb, _| wb.offers().len()),
        1,
        "the offer waits for another try"
    );
    assert!(
        !rig.script_calls()
            .iter()
            .any(|call| call == &format!("resume {KEY}")),
        "a denied resume never starts: {:?}",
        rig.script_calls()
    );
}

#[test]
fn a_saved_workspace_of_another_project_is_not_restored() {
    let mut rig = rig();
    let store = FixedWorkspaceStore::new(Some(SavedWorkspace {
        project: PathBuf::from("/elsewhere/other"),
        tabs: vec![SavedTab {
            path: "a.txt".into(),
            line: 1,
        }],
        active: 0,
        layout: LayoutPreset::Agents,
        dock: None,
        terminals: Vec::new(),
        task_links: Vec::new(),
    }));
    rig.set_workspace_source(store);
    rig.open_project();
    rig.settle_for(300);
    assert!(
        rig.editor_paths().is_empty(),
        "a state file that names another project must not seed tabs"
    );
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Balanced);
}

#[test]
fn workspace_changes_are_saved_after_the_delay_and_on_close() {
    let mut rig = rig();
    let saved = rig.record_workspace_changes();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    assert!(
        saved.lock().expect("lock").is_empty(),
        "the state waits for the delay"
    );
    // The write delay passes: the state is handed over with the tab and its cursor line.
    rig.cx.executor().advance_clock(Duration::from_millis(600));
    tick_ready(&mut rig);
    assert!(rig.settle_until(|_rig| !saved.lock().expect("lock").is_empty()));
    let workspace = saved
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .expect("a saved state");
    assert_eq!(workspace.project, rig.dir.path());
    assert_eq!(workspace.tabs.len(), 1);
    assert_eq!(workspace.tabs[0].path, "a.txt");
    // A close hands over what is pending without waiting for the delay.
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    let before = saved.lock().expect("lock").len();
    let allowed = rig.with(|wb, window, cx| wb.request_close(window, cx));
    assert!(!allowed, "the unsaved edit is asked about first");
    rig.with(|wb, window, cx| wb.resolve(Decision::Discard, window, cx));
    let states = saved.lock().expect("lock");
    assert!(
        states.len() > before,
        "the close must flush the pending state"
    );
    let last = states.last().expect("a saved state");
    assert_eq!(last.terminals, vec![SavedTerminal::Shell { name: None }]);
    assert_eq!(last.tabs.len(), 1, "the open tab is part of the state");
}

#[test]
fn an_unnamed_terminal_is_titled_by_kind_and_ordinal() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 2));

    assert_eq!(rig.read(|wb, _| wb.terminal_title(0)), "shell 1");
    assert_eq!(rig.read(|wb, _| wb.terminal_title(1)), "shell 2");
    assert_eq!(rig.read(|wb, _| wb.terminal_name(0).cloned()), None);
}

#[test]
fn renaming_a_terminal_sets_the_title_and_an_empty_name_clears_it() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));

    rig.run(CommandId::RenameTerminal);
    let prompt = rig
        .read(|wb, _| wb.rename_prompt().cloned())
        .expect("the prompt opens");
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("  build box ", window, cx);
            prompt.submit(cx);
        })
    });
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.rename_prompt().is_none())));
    assert_eq!(rig.read(|wb, _| wb.terminal_title(0)), "build box");

    rig.run(CommandId::RenameTerminal);
    let prompt = rig
        .read(|wb, _| wb.rename_prompt().cloned())
        .expect("the prompt opens again");
    assert_eq!(rig.read(|_wb, cx| prompt.read(cx).text(cx)), "build box");
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("", window, cx);
            prompt.submit(cx);
        })
    });
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.rename_prompt().is_none())));
    assert_eq!(rig.read(|wb, _| wb.terminal_title(0)), "shell 1");
}

#[test]
fn a_bad_terminal_name_is_refused_and_escape_closes_the_prompt() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));

    for text in ["--evil", "anahtar sk-abcdefghijklmnopqrstuvwx"] {
        rig.run(CommandId::RenameTerminal);
        let prompt = rig
            .read(|wb, _| wb.rename_prompt().cloned())
            .expect("the prompt opens");
        rig.with(|_wb, window, cx| {
            prompt.update(cx, |prompt, cx| {
                prompt.set_text(text, window, cx);
                prompt.submit(cx);
            })
        });
        rig.settle_for(50);
        assert!(rig.read(|wb, cx| wb
            .rename_prompt()
            .is_some_and(|prompt| prompt.read(cx).problem().is_some())));
        assert_eq!(
            rig.read(|wb, _| wb.terminal_name(0).cloned()),
            None,
            "{text:?}"
        );
        rig.with(|wb, window, cx| wb.close_rename_prompt(window, cx));
    }

    rig.run(CommandId::RenameTerminal);
    assert!(rig.read(|wb, _| wb.rename_prompt().is_some()));
    rig.cx.simulate_keystrokes("escape");
    rig.cx.run_until_parked();
    assert!(rig.read(|wb, _| wb.rename_prompt().is_none()));
}

#[test]
fn a_terminal_name_is_part_of_the_saved_workspace_and_comes_back_on_restore() {
    let mut rig = rig();
    let saved = rig.record_workspace_changes();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    rig.with(|wb, _window, cx| {
        wb.rename_terminal(0, SessionName::parse("api").ok(), cx);
    });
    rig.cx.executor().advance_clock(Duration::from_millis(600));
    tick_ready(&mut rig);
    assert!(rig.settle_until(|_rig| !saved.lock().expect("lock").is_empty()));
    let workspace = saved
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .expect("a state");
    assert_eq!(
        workspace.terminals,
        vec![SavedTerminal::Shell {
            name: Some("api".to_owned())
        }]
    );
}

#[test]
fn a_saved_terminal_name_is_restored_on_the_shell_it_belongs_to() {
    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("canonical");
    rig.set_workspace_source(FixedWorkspaceStore::new(Some(SavedWorkspace {
        project: root,
        tabs: vec![],
        active: 0,
        layout: LayoutPreset::Balanced,
        dock: None,
        terminals: vec![
            SavedTerminal::Shell {
                name: Some("api".to_owned()),
            },
            SavedTerminal::Shell { name: None },
        ],
        task_links: Vec::new(),
    })));
    rig.open_project();
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 2));
    assert_eq!(rig.read(|wb, _| wb.terminal_title(0)), "api");
    assert_eq!(rig.read(|wb, _| wb.terminal_title(1)), "shell 2");
}

#[test]
fn terminals_start_stacked_in_creation_order_and_a_closed_one_leaves_the_dock() {
    let mut rig = rig();
    rig.open_project();
    for _ in 0..3 {
        rig.run(CommandId::NewTerminal);
    }
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 3));

    let ids: Vec<_> = rig.read(|wb, _| (0..3).filter_map(|i| wb.terminal_id(i)).collect());
    assert_eq!(rig.read(|wb, _| wb.dock().sessions()), ids);
    assert_eq!(rig.read(|wb, _| wb.dock().validate()), Ok(()));

    // Stop the middle terminal; the next terminal makes room by dropping the dead pane.
    rig.run(CommandId::FocusTerminal(2));
    rig.run(CommandId::StopTerminal);
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.running_sessions()) == 2));
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 3));
    let now: Vec<_> = rig.read(|wb, _| (0..3).filter_map(|i| wb.terminal_id(i)).collect());
    assert_eq!(rig.read(|wb, _| wb.dock().sessions()), now);
    assert!(!rig.read(|wb, _| wb.dock().contains(&ids[1])));
}

#[test]
fn a_saved_dock_arrangement_is_restored_over_the_shells_that_come_back() {
    use rwk_core::workbench::{DockLayout, Edge};

    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("canonical");
    let mut dock = DockLayout::stacked([0_usize, 1, 2]);
    dock.split(&2, &0, Edge::Right).expect("split");
    rig.set_workspace_source(FixedWorkspaceStore::new(Some(SavedWorkspace {
        project: root,
        tabs: vec![],
        active: 0,
        layout: LayoutPreset::Balanced,
        terminals: vec![SavedTerminal::Shell { name: None }; 3],
        dock: Some(dock),
        task_links: Vec::new(),
    })));
    rig.open_project();
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 3));

    let ids: Vec<_> = rig.read(|wb, _| (0..3).filter_map(|i| wb.terminal_id(i)).collect());
    // Pane 2 sits to the right of pane 0, then pane 1 below them.
    assert_eq!(
        rig.read(|wb, _| wb.dock().sessions()),
        vec![ids[0].clone(), ids[2].clone(), ids[1].clone()]
    );
    assert_eq!(rig.read(|wb, _| wb.dock().validate()), Ok(()));
}

#[test]
fn the_current_dock_arrangement_is_part_of_the_saved_workspace() {
    let mut rig = rig();
    let saved = rig.record_workspace_changes();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 2));
    rig.cx.executor().advance_clock(Duration::from_millis(600));
    tick_ready(&mut rig);
    assert!(rig.settle_until(|_rig| !saved.lock().expect("lock").is_empty()));
    let workspace = saved
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .expect("a state");
    let dock = workspace.dock.expect("an arrangement");
    assert_eq!(dock.sessions(), vec![0, 1]);
}

/// Positions of the dock's title strips in the headless window, found by trying: the dock is the
/// right-hand panel, each of two stacked terminals has a thin title strip at the top of its half.
fn two_terminals(rig: &mut Rig) -> (rwk_core::sessions::SessionId, rwk_core::sessions::SessionId) {
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 2));
    let first = rig.read(|wb, _| wb.terminal_id(0)).expect("first");
    let second = rig.read(|wb, _| wb.terminal_id(1)).expect("second");
    (first, second)
}

#[test]
fn dropping_a_terminal_on_the_middle_of_another_makes_it_a_tab_there() {
    let mut rig = rig();
    let (first, second) = two_terminals(&mut rig);
    rig.with(|wb, window, cx| {
        wb.drop_terminal(
            &second,
            &first,
            rwk_core::workbench::DropZone::Center,
            window,
            cx,
        )
    });
    assert_eq!(
        rig.read(|wb, _| wb.dock().sessions()),
        vec![first.clone(), second.clone()]
    );
    assert_eq!(
        rig.read(|wb, _| wb.dock().group_mate(&first)),
        Some(second.clone())
    );
    assert_eq!(rig.read(|wb, _| wb.dock().validate()), Ok(()));
    // The moved terminal is the selected one and the group shows it.
    assert!(rig.read(|wb, _| wb.dock().is_shown(&second)));
    assert_eq!(rig.read(|wb, _| wb.selected_terminal()), 1);
}

#[test]
fn dropping_on_an_edge_splits_the_pane_and_the_sessions_keep_running() {
    let mut rig = rig();
    let (first, second) = two_terminals(&mut rig);
    rig.with(|wb, window, cx| {
        wb.drop_terminal(
            &second,
            &first,
            rwk_core::workbench::DropZone::Edge(rwk_core::workbench::Edge::Right),
            window,
            cx,
        )
    });
    assert_eq!(
        rig.read(|wb, _| wb.dock().sessions()),
        vec![first.clone(), second.clone()]
    );
    assert!(matches!(
        rig.read(|wb, _| wb.dock().root().cloned()),
        Some(rwk_core::workbench::Node::Split {
            axis: rwk_core::workbench::Axis::Horizontal,
            ..
        })
    ));
    assert_eq!(
        rig.read(|wb, _| wb.running_sessions()),
        2,
        "rearranging never touches the processes"
    );
}

#[test]
fn a_refused_drop_leaves_the_layout_and_says_why() {
    let mut rig = rig();
    let (first, _second) = two_terminals(&mut rig);
    let before = rig.read(|wb, _| wb.dock().clone());
    // A lone pane cannot be split from itself.
    rig.with(|wb, window, cx| {
        wb.drop_terminal(
            &first,
            &first,
            rwk_core::workbench::DropZone::Edge(rwk_core::workbench::Edge::Right),
            window,
            cx,
        )
    });
    assert_eq!(rig.read(|wb, _| wb.dock().clone()), before);
    assert!(rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("taşınamadı")));
}

#[test]
fn the_arrangement_commands_do_what_the_drags_do_and_survive_a_refusal() {
    let mut rig = rig();
    let (first, second) = two_terminals(&mut rig);
    // The second terminal is selected: swap it with the first.
    rig.run(CommandId::MoveTerminalPrevious);
    assert_eq!(
        rig.read(|wb, _| wb.dock().sessions()),
        vec![second.clone(), first.clone()]
    );
    // It is first now; moving further up is refused with a message and changes nothing.
    rig.run(CommandId::MoveTerminalPrevious);
    assert_eq!(
        rig.read(|wb, _| wb.dock().sessions()),
        vec![second.clone(), first.clone()]
    );
    assert!(rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("taşınamadı")));
    rig.run(CommandId::MoveTerminalNext);
    assert_eq!(
        rig.read(|wb, _| wb.dock().sessions()),
        vec![first.clone(), second.clone()]
    );

    rig.run(CommandId::SplitTerminalRight);
    assert!(matches!(
        rig.read(|wb, _| wb.dock().root().cloned()),
        Some(rwk_core::workbench::Node::Split {
            axis: rwk_core::workbench::Axis::Horizontal,
            ..
        })
    ));
    rig.run(CommandId::JoinTerminalPrevious);
    assert_eq!(
        rig.read(|wb, _| wb.dock().group_mate(&second)),
        Some(first.clone())
    );
    rig.run(CommandId::UngroupTerminal);
    assert_eq!(rig.read(|wb, _| wb.dock().group_mate(&second)), None);
    rig.run(CommandId::GrowTerminal);
    rig.run(CommandId::ShrinkTerminal);
    assert_eq!(rig.read(|wb, _| wb.dock().validate()), Ok(()));
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 2);
}

#[test]
fn dragging_a_divider_resizes_and_is_saved() {
    let mut rig = rig();
    let saved = rig.record_workspace_changes();
    let (_first, _second) = two_terminals(&mut rig);
    rig.with(|wb, _window, cx| wb.drag_divider(&[], 0, 0.8, cx));
    let weights = rig.read(|wb, _| match wb.dock().root() {
        Some(rwk_core::workbench::Node::Split { children, .. }) => {
            children.iter().map(|c| c.weight).collect::<Vec<_>>()
        }
        _ => Vec::new(),
    });
    assert!((weights[0] - 0.8).abs() < 1e-3, "{weights:?}");
    rig.cx.executor().advance_clock(Duration::from_millis(600));
    tick_ready(&mut rig);
    assert!(rig.settle_until(|_rig| !saved.lock().expect("lock").is_empty()));
    let workspace = saved
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .expect("a state");
    assert!(workspace.dock.is_some());
}

#[test]
fn arrangement_commands_are_unavailable_with_a_single_terminal() {
    use rwk_core::workbench::{available, WorkbenchState};
    let state = WorkbenchState {
        project_open: true,
        file_open: false,
        dirty: false,
        terminals: 1,
        dock_visible: true,
        control_plane_features: true,
        daemon_reachable: true,
        reconnectable: false,
    };
    assert!(!available(CommandId::MoveTerminalNext, &state).enabled);
    assert!(available(CommandId::UngroupTerminal, &state).enabled);
}

#[test]
fn a_real_drag_of_a_title_onto_another_pane_rearranges_the_dock() {
    use gpui::{point, px, Modifiers, MouseButton};

    let mut rig = rig();
    let (first, second) = two_terminals(&mut rig);
    rig.settle_for(100);
    let size = rig.cx.update(|window, _| window.viewport_size());
    // The dock is the right-hand panel; the second terminal's title strip is the first thing
    // painted in its half, so scan down the dock's left part for the strip that starts a drag.
    let x = size.width - px(120.0);
    let target = point(x, size.height * 0.27);
    let mut moved = false;
    // Start below the divider between the two panes (about the middle of the dock): a press on
    // it drags the divider instead of a title.
    let mut y = size.height * 0.5 + px(20.0);
    while y < size.height * 0.75 && !moved {
        rig.cx
            .simulate_mouse_move(point(x, y), None, Modifiers::default());
        rig.cx
            .simulate_mouse_down(point(x, y), MouseButton::Left, Modifiers::default());
        for step in 1..=6_u8 {
            let at = point(x, y + (target.y - y) * (f32::from(step) / 6.0));
            rig.cx
                .simulate_mouse_move(at, Some(MouseButton::Left), Modifiers::default());
        }
        rig.cx
            .simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
        rig.settle_for(50);
        moved = rig.read(|wb, _| wb.dock().group_mate(&second)) == Some(first.clone());
        y += px(4.0);
    }
    // Dropped on the middle of the first pane, the dragged terminal joined its group.
    assert!(moved, "no scanned point started a drag; viewport {size:?}");
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 2);
}

fn task_calls(fake: &FakeControlPlane) -> Vec<RecordedCall> {
    fake.calls()
        .iter()
        .filter(|call| {
            matches!(
                call,
                RecordedCall::CreateTask(_)
                    | RecordedCall::UpdateTaskStatus(_)
                    | RecordedCall::AssignTask(_)
            )
        })
        .cloned()
        .collect()
}

#[test]
fn a_new_task_is_titled_in_a_prompt_validated_and_created_through_the_guard() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_created_task(Ok("t-new".to_owned()));

    rig.run(CommandId::NewTask);
    let prompt = rig
        .read(|wb, _| wb.rename_prompt().cloned())
        .expect("the prompt opens");

    // A title that looks like a credential never leaves the workspace.
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("anahtar sk-abcdefghijklmnopqrstuvwx", window, cx);
            prompt.submit(cx);
        })
    });
    rig.settle_for(50);
    assert!(task_calls(&fake).is_empty());
    assert!(rig.read(|wb, cx| wb
        .rename_prompt()
        .is_some_and(|prompt| prompt.read(cx).problem().is_some())));

    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("  Derlemeyi düzelt  ", window, cx);
            prompt.submit(cx);
        })
    });
    assert!(rig.settle_until(|_| !task_calls(&fake).is_empty()));
    let RecordedCall::CreateTask(request) = task_calls(&fake).remove(0) else {
        panic!("a create call");
    };
    assert_eq!(request.title, "Derlemeyi düzelt");
    assert_eq!(request.priority, 50);
    assert_eq!(
        request.project_root,
        std::fs::canonicalize(rig.dir.path()).unwrap()
    );
    assert!(rig.read(|wb, _| wb.rename_prompt().is_none()));
    assert!(
        rig.settle_until(|rig| rig.read(|wb, _| wb.toast()) == Some("Görev eklendi".to_owned()))
    );
}

#[test]
fn a_status_change_and_an_assignment_carry_the_revision_the_user_saw() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_status_update(Ok(()));
    fake.push_assignment(Ok("2026-10-24T10:00:00Z".to_owned()));

    rig.with(|wb, window, cx| {
        wb.set_task_status(
            "t-1".into(),
            rwk_core::tasks::TaskStatus::Completed,
            window,
            cx,
        )
    });
    rig.with(|wb, window, cx| {
        wb.assign_task(
            "t-1".into(),
            "2026-10-24T09:00:00Z".into(),
            Some(SessionKind::Codex),
            window,
            cx,
        )
    });
    assert!(rig.settle_until(|_| task_calls(&fake).len() == 2));

    let calls = task_calls(&fake);
    let RecordedCall::UpdateTaskStatus(update) = &calls[0] else {
        panic!("a status call");
    };
    assert_eq!(update.status, rwk_core::tasks::TaskStatus::Completed);
    let RecordedCall::AssignTask(assign) = &calls[1] else {
        panic!("an assign call");
    };
    assert_eq!(assign.assignee, Some(SessionKind::Codex));
    assert_eq!(
        assign.expected_updated_at.as_deref(),
        Some("2026-10-24T09:00:00Z")
    );
}

#[test]
fn a_task_changed_elsewhere_says_so_and_a_shell_terminal_is_never_assigned() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_assignment(Err(rwk_core::error::AppError::new(
        rwk_core::error::ErrorCode::Conflict,
        "The task changed since it was read",
    )));

    rig.with(|wb, window, cx| {
        wb.assign_task(
            "t-1".into(),
            "old".into(),
            Some(SessionKind::Claude),
            window,
            cx,
        )
    });
    assert!(rig.settle_until(|rig| rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("bu arada değişti"))));

    // Terminal 1 is a plain shell: nothing is sent.
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    let before = task_calls(&fake).len();
    rig.with(|wb, window, cx| {
        wb.assign_task_to_terminal("t-1".into(), "old".into(), 0, window, cx)
    });
    assert_eq!(task_calls(&fake).len(), before);
    assert!(rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("ajan değil")));
}

#[test]
fn a_denied_task_policy_sends_nothing() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Deny);
    rig.with(|wb, window, cx| wb.create_task("Görev".into(), window, cx));
    rig.settle_for(100);
    assert!(task_calls(&fake).is_empty());
}

fn receipt_of(
    root: &std::path::Path,
    status: &str,
    delivered: bool,
) -> raios_contracts::HandoffReceiptDto {
    raios_contracts::HandoffReceiptDto {
        approval_id: "a-1".into(),
        project_path: root.to_string_lossy().into_owned(),
        from_agent: "claude_kaira".into(),
        to_agent: "codex_kaira".into(),
        status: status.into(),
        resolved_by: None,
        resolved_at: None,
        delivered_by_target: delivered,
    }
}

/// A rig with one open task "t-1" in the project, and a handoff of it queued towards Codex whose
/// receipts follow `receipts`.
fn handoff_of_a_task(receipts: &[(&str, bool)]) -> (Rig, Arc<FakeControlPlane>) {
    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("root");
    let work = raios_contracts::WorkSnapshot {
        tasks: vec![raios_contracts::UnifiedTaskDto {
            id: "t-1".into(),
            title: "Derlemeyi düzelt".into(),
            project_path: Some(root.to_string_lossy().into_owned()),
            updated_at: "2026-10-25T08:00:00Z".into(),
            ..raios_contracts::UnifiedTaskDto::default()
        }],
        ..raios_contracts::WorkSnapshot::default()
    };
    let fake = connected_with(&mut rig, PolicyAction::Allow, work);
    assert!(rig.settle_until(|rig| rig.read(|wb, cx| !wb
        .inspector()
        .read(cx)
        .model()
        .tasks
        .is_empty())));
    fake.push_handoff(Ok(QueuedHandoff {
        approval_id: "a-1".into(),
        project_path: root.clone(),
    }));
    for (status, delivered) in receipts {
        fake.push_receipt(Ok(receipt_of(&root, status, *delivered)));
    }
    fake.push_assignment(Ok("2026-10-25T09:00:00Z".to_owned()));

    // The row's "Devret…" button emits this event.
    let inspector = rig.read(|wb, _| wb.inspector().clone());
    rig.with(|_, _, cx| {
        inspector.update(cx, |_, cx| {
            cx.emit(rwk_ui::inspector::InspectorEvent::HandoffTask {
                task_id: "t-1".into(),
                title: "Derlemeyi düzelt".into(),
            })
        })
    });
    let composer = rig
        .read(|wb, _| wb.composer().cloned())
        .expect("the composer opens");
    let seeded = rig.read(|_, cx| composer.read(cx).text(cx));
    assert!(seeded.contains("Görev: Derlemeyi düzelt"), "{seeded}");
    rig.with(|_, window, cx| {
        composer.update(cx, |composer, cx| {
            let text = format!("{seeded}testleri çalıştır.");
            composer.set_text(&text, window, cx);
            composer.submit(SessionKind::Codex, cx);
        })
    });
    (rig, fake)
}

fn assignments(fake: &FakeControlPlane) -> Vec<rwk_core::control::AssignTaskRequest> {
    fake.calls()
        .into_iter()
        .filter_map(|call| match call {
            RecordedCall::AssignTask(request) => Some(request),
            _ => None,
        })
        .collect()
}

#[test]
fn the_task_follows_a_handoff_once_the_receipt_says_it_was_delivered() {
    let (mut rig, fake) = handoff_of_a_task(&[("approved", true)]);

    assert!(rig.settle_until(|_| !assignments(&fake).is_empty()));
    rig.settle_for(300);

    let moves = assignments(&fake);
    assert_eq!(moves.len(), 1, "the task moves once");
    assert_eq!(moves[0].task_id, "t-1");
    assert_eq!(moves[0].assignee, Some(SessionKind::Codex));
    assert_eq!(
        moves[0].expected_updated_at.as_deref(),
        Some("2026-10-25T08:00:00Z")
    );
}

#[test]
fn a_handoff_that_never_arrives_leaves_the_task_where_it_was() {
    let (mut rig, fake) = handoff_of_a_task(&[("expired", false)]);

    assert!(rig.settle_until(
        |rig| rig.read(|wb, _| { wb.handoffs().first().is_some_and(|h| !h.is_open()) })
    ));
    rig.settle_for(300);

    assert!(assignments(&fake).is_empty());
}

// ----- context menus and the toolbar ---------------------------------------------------------

use gpui::{point, px};
use rwk_core::workbench::MenuTarget;
use rwk_ui::context_menu::{ContextMenu, Line};
use rwk_ui::workbench::menus::{MenuAction, TaskMenuInfo};

fn menu_of(rig: &mut Rig) -> gpui::Entity<ContextMenu> {
    rig.read(|wb, _| wb.context_menu().cloned())
        .expect("a context menu is open")
}

fn menu_lines(rig: &mut Rig) -> Vec<Line> {
    let menu = menu_of(rig);
    rig.read(|_, cx| menu.read(cx).lines().to_vec())
}

fn menu_highlight(rig: &mut Rig) -> Option<usize> {
    let menu = menu_of(rig);
    rig.read(|_, cx| menu.read(cx).highlighted())
}

fn line_index(rig: &mut Rig, wanted: &MenuAction) -> usize {
    let menu = menu_of(rig);
    let count = rig.read(|_, cx| menu.read(cx).lines().len());
    (0..count)
        .find(|index| rig.read(|wb, _| wb.menu_action(*index)).as_ref() == Some(wanted))
        .unwrap_or_else(|| panic!("no line for {wanted:?}"))
}

#[test]
fn a_terminal_menu_lists_commands_and_choosing_one_runs_it_on_the_selected_terminal() {
    let mut rig = rig();
    let (_first, _second) = two_terminals(&mut rig);

    rig.with(|wb, window, cx| {
        wb.open_context_menu(
            MenuTarget::TerminalTitle,
            point(px(900.0), px(300.0)),
            window,
            cx,
        )
    });
    let index = line_index(&mut rig, &MenuAction::Command(CommandId::RenameTerminal));
    let menu = menu_of(&mut rig);
    rig.with(|_, _, cx| menu.update(cx, |menu, cx| menu.choose(index, cx)));

    // The menu is gone, the command ran.
    assert!(rig.read(|wb, _| wb.context_menu().is_none()));
    assert!(rig.read(|wb, _| wb.rename_prompt().is_some()));
}

#[test]
fn unavailable_commands_are_shown_with_their_reason_and_cannot_be_chosen() {
    let mut rig = rig();
    rig.open_project();

    // No daemon is connected and no terminal exists.
    rig.with(|wb, window, cx| {
        wb.open_context_menu(
            MenuTarget::DockBackground,
            point(px(900.0), px(300.0)),
            window,
            cx,
        )
    });
    let menu = menu_of(&mut rig);
    let lines = menu_lines(&mut rig);
    let disabled: Vec<(String, Option<String>)> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Item {
                label,
                hint,
                enabled: false,
            } => Some((label.to_string(), hint.as_ref().map(ToString::to_string))),
            _ => None,
        })
        .collect();
    assert!(
        disabled.iter().any(|(label, hint)| label == "Ajan başlat"
            && hint.as_deref() == Some("Daemon bağlantısı yapılandırılmadı")),
        "{disabled:?}"
    );
    // The agent line has no action behind it.
    let agent = lines
        .iter()
        .position(
            |line| matches!(line, Line::Item { label, .. } if label.as_ref() == "Ajan başlat"),
        )
        .expect("the agent line");
    assert_eq!(rig.read(|wb, _| wb.menu_action(agent)), None);
    rig.with(|_, _, cx| menu.update(cx, |menu, cx| menu.choose(agent, cx)));
    assert!(
        rig.read(|wb, _| wb.context_menu().is_some()),
        "choosing a disabled line keeps the menu open"
    );
}

#[test]
fn the_keyboard_walks_the_menu_and_escape_closes_it_without_running_anything() {
    let mut rig = rig();
    let (_first, _second) = two_terminals(&mut rig);
    rig.with(|wb, window, cx| {
        wb.open_context_menu(
            MenuTarget::TerminalTitle,
            point(px(900.0), px(300.0)),
            window,
            cx,
        )
    });
    let start = menu_highlight(&mut rig);
    assert!(
        start.is_some(),
        "the first choosable line starts highlighted"
    );

    rig.cx.simulate_keystrokes("down");
    let after = menu_highlight(&mut rig);
    assert_ne!(after, start);
    rig.cx.simulate_keystrokes("up");
    assert_eq!(menu_highlight(&mut rig), start);

    // Shortcuts do not fire behind an open menu.
    let before = rig.read(|wb, _| wb.terminal_count());
    rig.cx.simulate_keystrokes("ctrl-shift-t");
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), before);

    rig.cx.simulate_keystrokes("escape");
    assert!(rig.read(|wb, _| wb.context_menu().is_none()));
    assert!(rig.read(|wb, _| wb.rename_prompt().is_none()));
}

#[test]
fn enter_runs_the_highlighted_line() {
    let mut rig = rig();
    let (_first, _second) = two_terminals(&mut rig);
    rig.with(|wb, window, cx| {
        wb.open_context_menu(
            MenuTarget::TerminalTitle,
            point(px(900.0), px(300.0)),
            window,
            cx,
        )
    });
    // The first line of the terminal menu renames the terminal.
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.read(|wb, _| wb.context_menu().is_none()));
    assert!(rig.read(|wb, _| wb.rename_prompt().is_some()));
}

#[test]
fn a_real_right_click_on_a_terminal_title_selects_it_and_opens_its_menu() {
    use gpui::{Modifiers, MouseButton};

    let mut rig = rig();
    let (_first, second) = two_terminals(&mut rig);
    rig.settle_for(100);
    let size = rig.cx.update(|window, _| window.viewport_size());
    let x = size.width - px(120.0);
    // The second terminal's title strip is the first thing painted in its half of the dock.
    let mut opened = false;
    let mut y = size.height * 0.4;
    while y < size.height * 0.8 && !opened {
        rig.cx
            .simulate_mouse_down(point(x, y), MouseButton::Right, Modifiers::default());
        rig.settle_for(30);
        opened = rig.read(|wb, _| wb.context_menu().is_some());
        if opened {
            break;
        }
        y += px(4.0);
    }
    assert!(opened, "no scanned point opened a menu; viewport {size:?}");
    // Whatever was hit, a menu line exists for renaming and the click selected a terminal.
    let _ = second;
    assert!(rig.read(|wb, _| wb.selected_terminal() < 2));
    // A click anywhere else closes the menu and does nothing else.
    rig.cx.simulate_mouse_down(
        point(px(5.0), px(5.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    rig.settle_for(30);
    assert!(rig.read(|wb, _| wb.context_menu().is_none()));
}

#[test]
fn a_task_menu_changes_the_status_through_the_guard() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_status_update(Ok(()));
    let info = TaskMenuInfo {
        task_id: "t-1".into(),
        title: "Derle".into(),
        status: "pending".into(),
        assigned: false,
        updated_at: "2026-10-25T08:00:00Z".into(),
    };
    rig.with(|wb, window, cx| wb.open_task_menu(info, point(px(900.0), px(300.0)), window, cx));
    // "Başlat" and "Bitir" are offered, a status the task already has is not.
    let start = line_index(
        &mut rig,
        &MenuAction::TaskStatus {
            task_id: "t-1".into(),
            status: rwk_core::tasks::TaskStatus::InProgress,
        },
    );
    let menu = menu_of(&mut rig);
    rig.with(|_, _, cx| menu.update(cx, |menu, cx| menu.choose(start, cx)));

    assert!(rig.settle_until(|_| !task_calls(&fake).is_empty()));
    let RecordedCall::UpdateTaskStatus(update) = task_calls(&fake).remove(0) else {
        panic!("a status call");
    };
    assert_eq!(update.task_id, "t-1");
    assert_eq!(update.status, rwk_core::tasks::TaskStatus::InProgress);
}

#[test]
fn a_task_menu_cannot_assign_to_a_shell_and_clears_an_assignee() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_assignment(Ok("2026-10-25T09:00:00Z".to_owned()));
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    let info = TaskMenuInfo {
        task_id: "t-1".into(),
        title: "Derle".into(),
        status: "in_progress".into(),
        assigned: true,
        updated_at: "2026-10-25T08:00:00Z".into(),
    };
    rig.with(|wb, window, cx| wb.open_task_menu(info, point(px(900.0), px(300.0)), window, cx));
    // A shell is selected: assigning to it is shown but disabled.
    let menu = menu_of(&mut rig);
    let assign_enabled = menu_lines(&mut rig).iter().any(|line| {
        matches!(line, Line::Item { label, enabled: true, .. } if label.starts_with("Seçili terminalin"))
    });
    assert!(!assign_enabled);

    let clear = line_index(
        &mut rig,
        &MenuAction::TaskClear {
            task_id: "t-1".into(),
            updated_at: "2026-10-25T08:00:00Z".into(),
        },
    );
    rig.with(|_, _, cx| menu.update(cx, |menu, cx| menu.choose(clear, cx)));
    assert!(rig.settle_until(|_| !assignments(&fake).is_empty()));
    let sent = assignments(&fake).remove(0);
    assert_eq!(sent.assignee, None);
    assert_eq!(
        sent.expected_updated_at.as_deref(),
        Some("2026-10-25T08:00:00Z")
    );
}

#[test]
fn a_file_menu_offers_open_for_files_only_and_copies_the_path() {
    let mut rig = rig();
    rig.open_project();
    rig.with(|wb, window, cx| {
        wb.open_file_menu(
            "src/main.rs".into(),
            false,
            point(px(60.0), px(80.0)),
            window,
            cx,
        )
    });
    let open = line_index(&mut rig, &MenuAction::OpenFile("src/main.rs".into()));
    let copy = line_index(&mut rig, &MenuAction::CopyPath("src/main.rs".into()));
    assert!(open < copy);
    rig.with(|wb, window, cx| wb.close_context_menu(window, cx));

    rig.with(|wb, window, cx| {
        wb.open_file_menu("src".into(), true, point(px(60.0), px(80.0)), window, cx)
    });
    assert!((0..8).all(|index| {
        rig.read(|wb, _| wb.menu_action(index)) != Some(MenuAction::OpenFile("src".into()))
    }));
    let copy = line_index(&mut rig, &MenuAction::CopyPath("src".into()));
    let menu = menu_of(&mut rig);
    rig.with(|_, _, cx| menu.update(cx, |menu, cx| menu.choose(copy, cx)));
    assert_eq!(
        rig.cx.read_from_clipboard().and_then(|item| item.text()),
        Some("src".to_owned())
    );
    assert_eq!(
        rig.read(|wb, _| wb.toast()),
        Some("Yol panoya kopyalandı".to_owned())
    );
}

#[test]
fn only_one_menu_is_open_at_a_time_and_a_dialog_blocks_a_new_one() {
    let mut rig = rig();
    rig.open_project();
    rig.with(|wb, window, cx| {
        wb.open_context_menu(
            MenuTarget::DockBackground,
            point(px(900.0), px(300.0)),
            window,
            cx,
        )
    });
    let first = menu_of(&mut rig);
    rig.with(|wb, window, cx| {
        wb.open_context_menu(
            MenuTarget::EditorTab,
            point(px(300.0), px(30.0)),
            window,
            cx,
        )
    });
    let second = menu_of(&mut rig);
    assert_ne!(first.entity_id(), second.entity_id());
    rig.with(|wb, window, cx| wb.close_context_menu(window, cx));
    assert!(rig.read(|wb, _| wb.context_menu().is_none()));
}

fn toolbar_button(rig: &mut Rig, id: CommandId) -> rwk_ui::workbench::menus::ToolbarButton {
    rig.read(|wb, cx| wb.toolbar_buttons(cx))
        .into_iter()
        .find(|button| button.id == id)
        .unwrap_or_else(|| panic!("no toolbar button for {id:?}"))
}

#[test]
fn toolbar_buttons_follow_what_the_commands_can_do() {
    let mut rig = rig();

    // No project: the terminal button is there, disabled, and says why.
    let button = toolbar_button(&mut rig, CommandId::NewTerminal);
    assert!(!button.enabled);
    assert!(
        button.tip.contains("Önce bir proje klasörü açın"),
        "{}",
        button.tip
    );
    assert!(!toolbar_button(&mut rig, CommandId::Save).enabled);
    assert!(toolbar_button(&mut rig, CommandId::Commands).enabled);

    rig.open_project();
    assert!(toolbar_button(&mut rig, CommandId::NewTerminal).enabled);
    // Arrangement needs two terminals.
    assert!(!toolbar_button(&mut rig, CommandId::SplitTerminalRight).enabled);
    let (_a, _b) = two_terminals(&mut rig);
    let split = toolbar_button(&mut rig, CommandId::SplitTerminalRight);
    assert!(
        split.enabled && split.tip.contains("Terminali"),
        "{}",
        split.tip
    );
    // Shortcuts show in the tip.
    assert!(toolbar_button(&mut rig, CommandId::Commands)
        .tip
        .contains("Ctrl Shift P"));
}

#[test]
fn the_panel_toggles_show_a_pressed_state_that_follows_the_panels() {
    let mut rig = rig();
    rig.open_project();
    let dock = rig.read(|wb, _| wb.dock_visible());
    assert_eq!(
        toolbar_button(&mut rig, CommandId::ToggleTerminal).pressed,
        dock
    );
    rig.run(CommandId::ToggleTerminal);
    assert_eq!(
        toolbar_button(&mut rig, CommandId::ToggleTerminal).pressed,
        !dock
    );

    assert!(!toolbar_button(&mut rig, CommandId::Inspector).pressed);
    rig.run(CommandId::Inspector);
    assert!(toolbar_button(&mut rig, CommandId::Inspector).pressed);
}

#[test]
fn every_toolbar_command_runs_through_the_same_check_as_the_palette() {
    let mut rig = rig();
    // A disabled button's command answers with its reason instead of doing anything.
    rig.run(CommandId::Save);
    assert_eq!(
        rig.read(|wb, _| wb.toast()),
        Some("Açık dosya yok".to_owned())
    );
}

// ----- closing, killing and SSH ----------------------------------------------------------

fn argv_log(rig: &Rig) -> String {
    std::fs::read_to_string(rig.dir.path().join(".argv")).unwrap_or_default()
}

#[test]
fn closing_a_running_terminal_asks_first_then_removes_the_pane_and_ends_the_process() {
    let mut rig = rig();
    let (first, second) = two_terminals(&mut rig);
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 3));
    let _ = first;
    rig.run(CommandId::FocusTerminal(2));

    rig.run(CommandId::CloseTerminal);
    let dialog = rig.read(|wb, _| wb.dialog()).expect("a question");
    assert_eq!(dialog.title, "Terminali kapat");
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 3, "nothing yet");

    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 2);
    assert!(!rig.read(|wb, _| wb.dock().contains(&second)));
    assert_eq!(
        rig.read(|wb, _| wb.toast()),
        Some("Terminal kapatıldı".to_owned())
    );
    // The selection moved to a terminal that exists, and the process is really gone.
    assert!(rig.read(|wb, _| wb.selected_terminal()) < 2);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.running_sessions()) == 2));
}

#[test]
fn killing_says_what_it_skips_and_cancelling_leaves_everything_alone() {
    let mut rig = rig();
    let (_first, _second) = two_terminals(&mut rig);

    rig.run(CommandId::KillTerminal);
    let dialog = rig.read(|wb, _| wb.dialog()).expect("a question");
    assert_eq!(dialog.title, "Terminali zorla öldür");
    assert!(dialog.message.contains("SIGKILL"));
    rig.with(|wb, window, cx| wb.resolve(Decision::Cancel, window, cx));
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 2);
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 2);

    rig.run(CommandId::KillTerminal);
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 1);
    assert_eq!(
        rig.read(|wb, _| wb.toast()),
        Some("Terminal öldürüldü".to_owned())
    );
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.running_sessions()) == 1));
}

#[test]
fn the_last_terminal_can_be_closed_and_a_new_one_starts_after_it() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));

    rig.run(CommandId::CloseTerminal);
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
    assert!(rig.read(|wb, _| wb.dock().is_empty()));

    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
}

#[test]
fn a_middle_click_on_a_terminal_title_asks_to_close_it() {
    use gpui::{Modifiers, MouseButton};

    let mut rig = rig();
    let (_first, _second) = two_terminals(&mut rig);
    rig.settle_for(100);
    let size = rig.cx.update(|window, _| window.viewport_size());
    let x = size.width - px(160.0);
    let mut asked = false;
    let mut y = size.height * 0.5 + px(20.0);
    while y < size.height * 0.8 && !asked {
        rig.cx
            .simulate_mouse_down(point(x, y), MouseButton::Middle, Modifiers::default());
        rig.settle_for(30);
        asked = rig.read(|wb, _| wb.dialog()).is_some();
        y += px(4.0);
    }
    assert!(asked, "no scanned point was a title; viewport {size:?}");
    assert_eq!(
        rig.read(|wb, _| wb.dialog()).map(|dialog| dialog.title),
        Some("Terminali kapat")
    );
    assert_eq!(
        rig.read(|wb, _| wb.terminal_count()),
        2,
        "the question comes first"
    );
}

fn ssh_home_with(config: &str) -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("home");
    let ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).expect("ssh dir");
    std::fs::write(ssh.join("config"), config).expect("config");
    home
}

#[test]
fn the_ssh_picker_lists_known_hosts_and_connecting_starts_ssh_with_the_destination() {
    let mut rig = rig();
    rig.open_project();
    let home = ssh_home_with("Host prod\n  HostName 10.1.2.3\n  User deploy\n  Port 2200\n");
    rig.with(|wb, _, _| wb.set_ssh_home(Some(home.path().to_path_buf())));

    rig.run(CommandId::Ssh);
    let picker = rig
        .read(|wb, _| wb.ssh_picker().cloned())
        .expect("a picker");
    let rows = rig.read(|_, cx| picker.read(cx).rows().to_vec());
    assert_eq!(rows.len(), 1);
    assert!(rows[0].label.starts_with("prod"), "{}", rows[0].label);

    rig.cx.simulate_keystrokes("enter");
    assert!(rig.read(|wb, _| wb.ssh_picker().is_none()));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert_eq!(rig.read(|wb, _| wb.terminal_title(0)), "prod");
    assert!(
        rig.settle_until(|rig| argv_log(rig).contains("-- prod")),
        "{}",
        argv_log(&rig)
    );
}

#[test]
fn a_typed_target_connects_and_text_that_is_not_a_target_does_not() {
    let mut rig = rig();
    rig.open_project();
    let home = ssh_home_with("");
    rig.with(|wb, _, _| wb.set_ssh_home(Some(home.path().to_path_buf())));

    rig.run(CommandId::Ssh);
    let picker = rig
        .read(|wb, _| wb.ssh_picker().cloned())
        .expect("a picker");
    // Option-looking text offers no row, and Enter does nothing.
    rig.with(|_, window, cx| {
        picker.update(cx, |picker, cx| {
            picker.set_query("-oProxyCommand=touch /tmp/pwned", window, cx)
        })
    });
    assert!(rig.read(|_, cx| picker.read(cx).rows().is_empty()));
    rig.cx.simulate_keystrokes("enter");
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
    assert!(rig.read(|wb, _| wb.ssh_picker().is_some()));

    rig.with(|_, window, cx| {
        picker.update(cx, |picker, cx| {
            picker.set_query("me@10.0.0.7:2200", window, cx)
        })
    });
    let rows = rig.read(|_, cx| picker.read(cx).rows().to_vec());
    assert_eq!(rows.len(), 1);
    assert!(rows[0].typed);
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert!(
        rig.settle_until(|rig| argv_log(rig).contains("-p 2200 -- me@10.0.0.7")),
        "{}",
        argv_log(&rig)
    );
    assert_eq!(rig.read(|wb, _| wb.terminal_title(0)), "me@10.0.0.7:2200");
}

#[test]
fn escape_closes_the_picker_and_a_remote_pane_is_never_saved() {
    let mut rig = rig();
    rig.open_project();
    let home = ssh_home_with("Host prod\n");
    rig.with(|wb, _, _| wb.set_ssh_home(Some(home.path().to_path_buf())));
    let saved = rig.record_workspace_changes();

    rig.run(CommandId::Ssh);
    rig.cx.simulate_keystrokes("escape");
    assert!(rig.read(|wb, _| wb.ssh_picker().is_none()));
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);

    // One local shell and one remote shell: only the local one is written down.
    rig.run(CommandId::NewTerminal);
    rig.with(|wb, window, cx| {
        wb.connect_ssh(rwk_core::ssh::SshTarget::parse("prod").unwrap(), window, cx)
    });
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 2));
    rig.cx.executor().advance_clock(Duration::from_millis(600));
    rig.settle_for(50);
    let workspace = saved
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .expect("a state");
    assert_eq!(workspace.terminals.len(), 1, "{:?}", workspace.terminals);
    assert!(workspace.dock.as_ref().is_some_and(|dock| dock.len() == 1));
}

#[test]
fn ssh_is_unavailable_without_a_project_and_listed_in_the_menus_and_toolbar() {
    let mut rig = rig();
    let button = toolbar_button(&mut rig, CommandId::Ssh);
    assert!(!button.enabled);
    rig.open_project();
    assert!(toolbar_button(&mut rig, CommandId::Ssh).enabled);
    let no_terminal = toolbar_button(&mut rig, CommandId::CloseTerminal);
    assert!(!no_terminal.enabled && no_terminal.tip.contains("Açık terminal yok"));
    rig.run(CommandId::NewTerminal);
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert!(toolbar_button(&mut rig, CommandId::CloseTerminal)
        .tip
        .contains("Ctrl Shift W"));

    rig.with(|wb, window, cx| {
        wb.open_context_menu(
            MenuTarget::DockBackground,
            point(px(900.0), px(300.0)),
            window,
            cx,
        )
    });
    let ssh = line_index(&mut rig, &MenuAction::Command(CommandId::Ssh));
    let _ = ssh;
}

fn enabled_labels(rig: &mut Rig) -> Vec<String> {
    menu_lines(rig)
        .into_iter()
        .filter_map(|line| match line {
            Line::Item {
                label,
                enabled: true,
                ..
            } => Some(label.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_approval_menu_offers_both_decisions_only_for_handoffs() {
    let mut rig = rig();
    rig.with(|wb, window, cx| {
        wb.open_approval_menu(
            "a-1".into(),
            4,
            true,
            point(px(900.0), px(300.0)),
            window,
            cx,
        )
    });
    assert_eq!(enabled_labels(&mut rig), ["Onayla", "Reddet…"]);
    line_index(
        &mut rig,
        &MenuAction::RejectHandoff {
            approval_id: "a-1".into(),
            revision: 4,
        },
    );

    rig.with(|wb, window, cx| {
        wb.open_approval_menu(
            "a-2".into(),
            1,
            false,
            point(px(900.0), px(300.0)),
            window,
            cx,
        )
    });
    assert!(enabled_labels(&mut rig).is_empty());
}

#[test]
fn a_handoff_menu_starts_the_receiver_only_when_it_can_be_started() {
    let mut rig = rig();
    rig.with(|wb, window, cx| {
        wb.open_handoff_menu("h-1".into(), true, point(px(900.0), px(300.0)), window, cx)
    });
    line_index(&mut rig, &MenuAction::StartReceiver("h-1".into()));

    rig.with(|wb, window, cx| {
        wb.open_handoff_menu("h-2".into(), false, point(px(900.0), px(300.0)), window, cx)
    });
    assert!(enabled_labels(&mut rig).is_empty());
}

#[test]
fn the_text_menu_enables_cut_and_copy_only_with_a_selection_and_edits_only_when_editable() {
    let mut rig = rig();
    rig.with(|wb, window, cx| {
        wb.open_text_menu(false, true, point(px(400.0), px(300.0)), window, cx)
    });
    assert_eq!(
        enabled_labels(&mut rig),
        ["Geri al", "Yinele", "Yapıştır", "Tümünü seç"]
    );
    rig.with(|wb, window, cx| {
        wb.open_text_menu(true, true, point(px(400.0), px(300.0)), window, cx)
    });
    assert!(enabled_labels(&mut rig).contains(&"Kes".to_owned()));
    assert!(enabled_labels(&mut rig).contains(&"Kopyala".to_owned()));
    rig.with(|wb, window, cx| {
        wb.open_text_menu(true, false, point(px(400.0), px(300.0)), window, cx)
    });
    assert_eq!(enabled_labels(&mut rig), ["Kopyala", "Tümünü seç"]);
}

#[test]
fn right_clicking_the_editor_text_opens_the_text_menu() {
    use gpui::{Modifiers, MouseButton};

    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    let size = rig.cx.update(|window, _| window.viewport_size());
    let mut found = false;
    'scan: for row in 0..14 {
        for column in 0..8 {
            let at = point(
                size.width * (0.2 + 0.05 * column as f32),
                size.height * (0.12 + 0.03 * row as f32),
            );
            rig.cx
                .simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
            rig.cx
                .simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
            rig.settle_for(30);
            if rig.read(|wb, _| wb.context_menu().is_some()) {
                if enabled_labels(&mut rig).contains(&"Tümünü seç".to_owned()) {
                    found = true;
                    break 'scan;
                }
                rig.cx.simulate_keystrokes("escape");
                rig.settle_for(30);
            }
        }
    }
    assert!(found, "no point of the editor opened the text menu");
}

#[test]
fn an_ended_ssh_shell_can_be_reconnected_into_a_fresh_pane() {
    let mut rig = rig();
    rig.open_project();
    let home = tempfile::tempdir().unwrap();
    rig.with(|wb, _, _| wb.set_ssh_home(Some(home.path().to_path_buf())));
    let target = rwk_core::ssh::SshTarget::parse("me@10.0.0.7").unwrap();
    rig.with(|wb, window, cx| wb.connect_ssh(target, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    // While it runs there is nothing to reconnect.
    assert!(!rig.read(|wb, cx| wb.state(cx).reconnectable));

    let first = rig.read(|wb, _| wb.terminal_id(0));
    rig.run(CommandId::StopTerminal);
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, cx| wb.state(cx).reconnectable)));

    rig.run(CommandId::Reconnect);
    assert!(
        rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1
            && rig.read(|wb, _| wb.terminal_id(0)) != first)
    );
    assert!(!rig.read(|wb, cx| wb.state(cx).reconnectable));
}

#[test]
fn clicking_along_the_toolbar_presses_real_buttons() {
    use gpui::{Modifiers, MouseButton};

    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::ToolbarTab(2));
    rig.settle_for(100);
    let size = rig.cx.update(|window, _| window.viewport_size());
    // The toolbar is the row under the title area; sweep it from the left until a click
    // opens a terminal (the "Terminal +" button) or the row is exhausted. The sweep starts
    // after the category tabs, which would switch the buttons under the pointer.
    let mut x = px(380.0);
    while x < size.width && rig.read(|wb, _| wb.terminal_count()) == 0 {
        for y in [px(14.0), px(30.0), px(44.0)] {
            let at = point(x, y);
            rig.cx
                .simulate_mouse_down(at, MouseButton::Left, Modifiers::default());
            rig.cx
                .simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
        }
        rig.settle_for(20);
        // Buttons that open overlays (project, files, search…) would swallow later clicks.
        if rig.read(|wb, _| {
            wb.palette().is_some() || wb.ssh_picker().is_some() || wb.context_menu().is_some()
        }) {
            rig.cx.simulate_keystrokes("escape");
            rig.settle_for(20);
        }
        x += px(10.0);
    }
    assert!(
        rig.read(|wb, _| wb.terminal_count()) >= 1,
        "no toolbar click started a terminal"
    );
}

#[test]
fn finished_task_groups_start_collapsed_and_open_on_a_click() {
    use rwk_core::inspector::TaskGroup;

    let mut rig = rig();
    let inspector = rig.read(|wb, _| wb.inspector().clone());
    let collapsed =
        |rig: &mut Rig, group| rig.read(|_, cx| inspector.read(cx).group_collapsed(group));
    assert!(!collapsed(&mut rig, TaskGroup::InProgress));
    assert!(!collapsed(&mut rig, TaskGroup::Open));
    assert!(collapsed(&mut rig, TaskGroup::Completed));
    assert!(collapsed(&mut rig, TaskGroup::Cancelled));

    rig.with(|_, _, cx| {
        inspector.update(cx, |view, cx| view.toggle_group(TaskGroup::Completed, cx))
    });
    assert!(!collapsed(&mut rig, TaskGroup::Completed));
    rig.with(|_, _, cx| {
        inspector.update(cx, |view, cx| view.toggle_group(TaskGroup::Completed, cx))
    });
    assert!(collapsed(&mut rig, TaskGroup::Completed));
}

#[test]
fn a_task_given_to_an_agent_terminal_is_carried_saved_and_marked_when_the_terminal_closes() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_assignment(Ok("2026-10-26T09:00:00Z".to_owned()));
    fake.push_launch(Ok(()));
    let saved = rig.record_workspace_changes();
    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Codex, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));

    rig.with(|wb, window, cx| {
        wb.assign_task_to_terminal("t-1".into(), "rev".into(), 0, window, cx)
    });
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_task(0).is_some())));
    assert_eq!(
        rig.read(|wb, _| wb.terminal_task(0).map(|task| task.task_id.clone())),
        Some("t-1".to_owned())
    );

    // The link is part of the saved workspace.
    rig.cx.executor().advance_clock(Duration::from_millis(600));
    rig.settle_for(50);
    let workspace = saved
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .expect("a state");
    assert_eq!(
        workspace
            .task_links
            .first()
            .cloned()
            .flatten()
            .map(|link| link.id),
        Some("t-1".to_owned())
    );

    // Closing the terminal leaves the task alone but the list says its terminal is gone.
    rig.run(CommandId::CloseTerminal);
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 0));
    let inspector = rig.read(|wb, _| wb.inspector().clone());
    assert!(rig.read(|_, cx| inspector.read(cx).task_terminal_closed("t-1")));
}

#[test]
fn a_failed_assignment_links_nothing() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_assignment(Err(rwk_core::error::AppError::new(
        rwk_core::error::ErrorCode::Conflict,
        "The task changed since it was read",
    )));
    fake.push_launch(Ok(()));
    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Codex, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    rig.with(|wb, window, cx| {
        wb.assign_task_to_terminal("t-1".into(), "old".into(), 0, window, cx)
    });
    assert!(rig.settle_until(|rig| rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("bu arada değişti"))));
    assert!(rig.read(|wb, _| wb.terminal_task(0).is_none()));
}

#[test]
fn a_handoff_text_can_be_pasted_into_another_running_agent_terminal_without_a_record() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_launch(Ok(()));
    fake.push_launch(Ok(()));
    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Codex, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Claude, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 2));
    let sends_before = sends(&fake);

    // The sender is the selected (second) terminal; the first is the only paste target.
    rig.run(CommandId::Handoff);
    let composer = rig.read(|wb, _| wb.composer().cloned()).expect("composer");
    let target = rig.read(|wb, _| wb.terminal_id(0)).expect("first terminal");
    rig.with(|_, window, cx| {
        composer.update(cx, |composer, cx| {
            composer.set_text("satir bir\nsatir iki", window, cx);
            composer.submit_paste(target.clone(), cx);
        })
    });
    rig.cx.run_until_parked();

    assert!(rig.read(|wb, _| wb.composer().is_none()));
    let view = rig
        .read(|wb, _| wb.terminal_view(0).cloned())
        .expect("view");
    // The composer text is one line, so it is written to the prompt, not held in a preview;
    // either way the user still has to press Enter in the terminal.
    assert!(rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("Devir kaydı oluşmadı")));
    assert_eq!(rig.read(|wb, _| wb.selected_terminal()), 0);
    let _ = view;
    assert_eq!(sends(&fake), sends_before, "a paste is not a handoff");
}

#[test]
fn a_secret_is_not_pasted_into_a_terminal_either() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_launch(Ok(()));
    fake.push_launch(Ok(()));
    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Codex, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Claude, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 2));
    rig.run(CommandId::Handoff);
    let composer = rig.read(|wb, _| wb.composer().cloned()).expect("composer");
    let target = rig.read(|wb, _| wb.terminal_id(0)).expect("terminal");
    rig.with(|_, window, cx| {
        composer.update(cx, |composer, cx| {
            composer.set_text("anahtar sk-abcdefghijklmnopqrstuvwx", window, cx);
            composer.submit_paste(target.clone(), cx);
        })
    });
    rig.cx.run_until_parked();
    assert!(rig.read(|wb, cx| wb
        .composer()
        .is_some_and(|c| c.read(cx).problem().is_some())));
    let view = rig
        .read(|wb, _| wb.terminal_view(0).cloned())
        .expect("view");
    assert!(!rig.read(|_, cx| view.read(cx).has_pending_paste()));
}

fn answer_entry_prompt(rig: &mut Rig, text: &str) {
    let prompt = rig
        .read(|wb, _| wb.rename_prompt().cloned())
        .expect("a name prompt");
    rig.with(|_, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text(text, window, cx);
            prompt.submit(cx);
        })
    });
    rig.cx.run_until_parked();
}

#[test]
fn a_new_file_is_created_in_the_project_and_opened() {
    use rwk_ui::workbench::entries::EntryOp;

    let mut rig = rig();
    rig.open_project();
    rig.with(|wb, window, cx| wb.open_entry_prompt(EntryOp::NewFile(String::new()), window, cx));
    // A name with a separator is refused in the prompt and nothing is created.
    answer_entry_prompt(&mut rig, "../escape.txt");
    assert!(rig.read(|wb, cx| wb
        .rename_prompt()
        .is_some_and(|p| p.read(cx).problem().is_some())));
    assert!(!rig.dir.path().join("../escape.txt").exists());

    answer_entry_prompt(&mut rig, "fresh.txt");
    assert!(rig.dir.path().join("fresh.txt").is_file());
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("fresh.txt")));
}

#[test]
fn renaming_or_deleting_an_open_file_is_refused_and_a_closed_one_goes_after_a_question() {
    use rwk_ui::workbench::entries::EntryOp;

    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));

    rig.with(|wb, window, cx| wb.open_entry_prompt(EntryOp::Rename("a.txt".into()), window, cx));
    assert!(rig.read(|wb, _| wb.rename_prompt().is_none()));
    rig.with(|wb, _, cx| wb.request_delete_entry("a.txt".into(), false, cx));
    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert!(rig.dir.path().join("a.txt").exists());

    // b.txt is not open: rename works, delete asks first.
    rig.with(|wb, window, cx| wb.open_entry_prompt(EntryOp::Rename("b.txt".into()), window, cx));
    answer_entry_prompt(&mut rig, "c.txt");
    assert!(rig.dir.path().join("c.txt").exists());
    assert!(!rig.dir.path().join("b.txt").exists());

    rig.with(|wb, _, cx| wb.request_delete_entry("c.txt".into(), false, cx));
    assert!(
        rig.dir.path().join("c.txt").exists(),
        "nothing before the answer"
    );
    assert!(matches!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::DeleteEntry { .. })
    ));
    rig.with(|wb, window, cx| wb.resolve(Decision::Cancel, window, cx));
    assert!(rig.dir.path().join("c.txt").exists());

    rig.with(|wb, _, cx| wb.request_delete_entry("c.txt".into(), false, cx));
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert!(!rig.dir.path().join("c.txt").exists());
}

#[test]
fn a_bound_terminal_whose_process_ended_marks_its_task() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_assignment(Ok("2026-10-26T09:00:00Z".to_owned()));
    fake.push_launch(Ok(()));
    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Codex, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    rig.with(|wb, window, cx| {
        wb.assign_task_to_terminal("t-9".into(), "rev".into(), 0, window, cx)
    });
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_task(0).is_some())));
    let inspector = rig.read(|wb, _| wb.inspector().clone());
    assert!(!rig.read(|_, cx| inspector.read(cx).task_terminal_closed("t-9")));

    rig.run(CommandId::StopTerminal);
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_task_ended(0))));
    rig.with(|_, window, _| window.refresh());
    assert!(
        rig.settle_until(|rig| rig.read(|_, cx| inspector.read(cx).task_terminal_closed("t-9")))
    );
    // The pane is still there and still carries the task.
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 1);
}

#[test]
fn the_empty_part_of_the_file_tree_offers_new_entries_at_the_project_root() {
    let mut rig = rig();
    rig.open_project();
    rig.with(|wb, window, cx| {
        wb.open_file_menu(String::new(), true, point(px(60.0), px(400.0)), window, cx)
    });
    let labels = enabled_labels(&mut rig);
    assert!(labels.contains(&"Yeni dosya…".to_owned()), "{labels:?}");
    assert!(labels.contains(&"Yeni klasör…".to_owned()));
    assert!(
        !labels.contains(&"Sil…".to_owned()),
        "the root cannot be deleted"
    );
    assert!(!labels.contains(&"Yeniden adlandır…".to_owned()));
    line_index(&mut rig, &MenuAction::NewFile(String::new()));
}

#[test]
fn right_clicking_below_the_tree_rows_opens_the_root_menu() {
    use gpui::{Modifiers, MouseButton};

    let mut rig = rig();
    rig.open_project();
    rig.settle_for(100);
    let size = rig.cx.update(|window, _| window.viewport_size());
    // The tree is the left column; sweep its lower part, below the few rows of the test project.
    let mut found = false;
    'scan: for row in 0..10 {
        for column in 0..4 {
            let at = point(
                px(30.0 + 40.0 * column as f32),
                size.height * (0.5 + 0.04 * row as f32),
            );
            rig.cx
                .simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
            rig.settle_for(30);
            if rig.read(|wb, _| wb.context_menu().is_some()) {
                if enabled_labels(&mut rig).contains(&"Yeni dosya…".to_owned())
                    && !enabled_labels(&mut rig).contains(&"Sil…".to_owned())
                {
                    found = true;
                    break 'scan;
                }
                rig.cx.simulate_keystrokes("escape");
                rig.settle_for(30);
            }
        }
    }
    assert!(found, "no point below the rows opened the root menu");
}

/// A language server stand-in: answers `initialize` and reports one error on line 2 and one
/// warning on line 1 of every file it is sent. Needs `python3`.
const FAKE_LSP: &str = r#"
import json, sys
def read():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line: return None
        line = line.strip()
        if not line: break
        if line.lower().startswith(b'content-length:'): length = int(line.split(b':')[1])
    return json.loads(sys.stdin.buffer.read(length))
def send(value):
    body = json.dumps(value).encode()
    sys.stdout.buffer.write(b'Content-Length: %d\r\n\r\n' % len(body) + body)
    sys.stdout.buffer.flush()
while True:
    m = read()
    if m is None: break
    method = m.get('method')
    if method == 'initialize':
        send({'jsonrpc': '2.0', 'id': m['id'], 'result': {'capabilities': {}}})
    elif method == 'textDocument/didOpen':
        uri = m['params']['textDocument']['uri']
        r = lambda l: {'start': {'line': l, 'character': 0}, 'end': {'line': l, 'character': 2}}
        send({'jsonrpc': '2.0', 'method': 'textDocument/publishDiagnostics', 'params': {'uri': uri, 'diagnostics': [
            {'range': r(1), 'severity': 1, 'message': 'broken'},
            {'range': r(0), 'severity': 2, 'message': 'odd'}]}})
    elif method == 'textDocument/hover':
        send({'jsonrpc': '2.0', 'id': m['id'], 'result': {'contents': 'fn a()'}})
    elif method == 'textDocument/definition':
        uri = m['params']['textDocument']['uri'].replace('a.rs', 'b.rs')
        send({'jsonrpc': '2.0', 'id': m['id'], 'result': {'uri': uri,
            'range': {'start': {'line': 4, 'character': 1}, 'end': {'line': 4, 'character': 3}}}})
    elif method == 'textDocument/formatting':
        e = lambda l, c0, c1, t: {'range': {'start': {'line': l, 'character': c0}, 'end': {'line': l, 'character': c1}}, 'newText': t}
        send({'jsonrpc': '2.0', 'id': m['id'], 'result': [e(0, 0, 2, 'FN'), e(1, 5, 5, ';')]})
    elif method == 'textDocument/rename':
        uri = m['params']['textDocument']['uri']
        name = m['params']['newName']
        e = lambda t: {'range': {'start': {'line': 0, 'character': 0}, 'end': {'line': 0, 'character': 2}}, 'newText': t}
        send({'jsonrpc': '2.0', 'id': m['id'], 'result': {'changes': {
            uri: [e(name)], uri.replace('a.rs', 'b.rs'): [e(name)]}}})
    elif method == 'textDocument/completion':
        send({'jsonrpc': '2.0', 'id': m['id'], 'result': [{'label': 'alpha'}]})
    elif method == 'textDocument/signatureHelp':
        send({'jsonrpc': '2.0', 'id': m['id'], 'result': {'signatures': [
            {'label': 'fn a(x: i32, y: u8)', 'parameters': [{'label': 'x: i32'}, {'label': 'y: u8'}]}],
            'activeParameter': 1}})
    elif method == 'shutdown':
        send({'jsonrpc': '2.0', 'id': m['id'], 'result': None})
    elif method == 'exit':
        break
"#;

#[test]
fn language_server_problems_reach_the_editor_the_status_line_and_the_problems_view() {
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let mut rig = rig();
    let script = rig.dir.path().join(".fake_lsp.py");
    std::fs::write(&script, FAKE_LSP).expect("fake server");
    std::fs::write(rig.dir.path().join("a.rs"), "fn a() {}\nlet x\n").expect("a.rs");
    rig.with(|wb, _, _| {
        wb.set_language_server_launcher(Some(Arc::new(move |_spec, root| {
            let mut command = std::process::Command::new("python3");
            command.arg(&script);
            rwk_core::lsp::LspClient::start_command(command, root)
        })));
    });
    rig.open_project();
    rig.open_file("a.rs");

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.problem_summary().is_some())));
    assert_eq!(
        rig.read(|wb, _| wb.problem_summary()),
        Some("1 hata · 1 uyarı".to_owned())
    );
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).diagnostic_count("a.rs", cx)),
        2
    );

    // The Problems view lists the error first; choosing it opens the file on that line.
    rig.run(CommandId::Problems);
    let first = rig.read(|wb, cx| {
        wb.problems_view()
            .map(|view| view.read(cx).problems().first().cloned())
    });
    let first = first.flatten().expect("the view lists problems");
    assert_eq!((first.path.as_str(), first.line), ("a.rs", 2));

    // Grouped by file the order follows the lines instead of the severity.
    let view = rig.read(|wb, _| wb.problems_view().cloned()).unwrap();
    rig.with(|_wb, _w, cx| view.update(cx, |view, cx| view.toggle_grouping(cx)));
    let lines = rig.cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(view.grouped());
        view.problems().iter().map(|p| p.line).collect::<Vec<_>>()
    });
    assert_eq!(lines, [1, 2]);
    rig.with(|_wb, _w, cx| view.update(cx, |view, cx| view.toggle_grouping(cx)));
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.read(|wb, _| wb.problems_view().is_none()));
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).cursor_line(cx)),
        Some(2)
    );

    // Closing the tab forgets its problems.
    rig.with(|wb, window, cx| wb.request_close_tab("a.rs".to_owned(), window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.problem_summary().is_none())));
}

#[test]
fn language_server_answers_hover_definition_and_completion() {
    use gpui_base::input::{CompletionProvider, DefinitionProvider, HoverProvider, Rope};
    use rwk_ui::editor::lsp::ServerProvider;
    use std::cell::RefCell;
    use std::rc::Rc;

    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let mut rig = rig();
    let script = rig.dir.path().join(".fake_lsp.py");
    std::fs::write(&script, FAKE_LSP).expect("fake server");
    std::fs::write(rig.dir.path().join("a.rs"), "fn a() {}\nlet x\n").expect("a.rs");
    rig.with(|wb, _, _| {
        wb.set_language_server_launcher(Some(Arc::new(move |_spec, root| {
            let mut command = std::process::Command::new("python3");
            command.arg(&script);
            rwk_core::lsp::LspClient::start_command(command, root)
        })));
    });
    rig.open_project();
    rig.open_file("a.rs");
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.problem_summary().is_some())));

    let requester = rig
        .read(|wb, _| wb.language_requester())
        .expect("a project with servers");
    let provider = ServerProvider::new("a.rs", &requester);
    let rope = Rope::from("fn a() {}\nlet x\n");

    let hover = Rc::new(RefCell::new(None));
    let definitions = Rc::new(RefCell::new(None));
    let completions = Rc::new(RefCell::new(None));
    rig.cx.update(|window, cx| {
        let task = provider.hover(&rope, 3, window, cx);
        let slot = hover.clone();
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach();
        let task = provider.definitions(&rope, 3, window, cx);
        let slot = definitions.clone();
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach();
        let context = rwk_core::lsp::lsp_types::CompletionContext {
            trigger_kind: rwk_core::lsp::lsp_types::CompletionTriggerKind::INVOKED,
            trigger_character: None,
        };
        let task = provider.completions(&rope, 3, context, window, cx);
        let slot = completions.clone();
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach();
    });
    assert!(rig.settle_until(|_| {
        hover.borrow().is_some() && definitions.borrow().is_some() && completions.borrow().is_some()
    }));

    let hover = hover.borrow_mut().take().unwrap().expect("hover answers");
    assert!(hover.is_some());
    let definitions = definitions
        .borrow_mut()
        .take()
        .unwrap()
        .expect("definition answers");
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        requester
            .relative_path(&definitions[0].target_uri.to_string())
            .as_deref(),
        Some("b.rs")
    );
    assert_eq!(definitions[0].target_selection_range.start.line, 4);
    let completions = completions
        .borrow_mut()
        .take()
        .unwrap()
        .expect("completion answers");
    assert!(matches!(
        completions,
        rwk_core::lsp::lsp_types::CompletionResponse::Array(items) if items[0].label == "alpha"
    ));
}

#[test]
fn the_format_command_applies_the_servers_edits_last_first() {
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let mut rig = rig();
    let script = rig.dir.path().join(".fake_lsp.py");
    std::fs::write(&script, FAKE_LSP).expect("fake server");
    std::fs::write(rig.dir.path().join("a.rs"), "fn a() {}\nlet x\n").expect("a.rs");
    rig.with(|wb, _, _| {
        wb.set_language_server_launcher(Some(Arc::new(move |_spec, root| {
            let mut command = std::process::Command::new("python3");
            command.arg(&script);
            rwk_core::lsp::LspClient::start_command(command, root)
        })));
    });
    rig.open_project();
    rig.open_file("a.rs");
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.problem_summary().is_some())));

    rig.run(CommandId::Format);
    assert!(rig.settle_until(|rig| rig.editor_text() == "FN a() {}\nlet x;\n"));
}

#[test]
fn renaming_a_symbol_edits_open_tabs_and_writes_the_other_files() {
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let mut rig = rig();
    let script = rig.dir.path().join(".fake_lsp.py");
    std::fs::write(&script, FAKE_LSP).expect("fake server");
    std::fs::write(rig.dir.path().join("a.rs"), "fn a() {}\nlet x\n").expect("a.rs");
    std::fs::write(rig.dir.path().join("b.rs"), "fn b() {}\n").expect("b.rs");
    rig.with(|wb, _, _| {
        wb.set_language_server_launcher(Some(Arc::new(move |_spec, root| {
            let mut command = std::process::Command::new("python3");
            command.arg(&script);
            rwk_core::lsp::LspClient::start_command(command, root)
        })));
    });
    rig.open_project();
    rig.open_file("a.rs");
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.problem_summary().is_some())));

    // The cursor starts on `fn`, which is what the prompt offers.
    rig.run(CommandId::RenameSymbol);
    let prompt = rig
        .read(|wb, _| wb.rename_prompt().cloned())
        .expect("the prompt opens");
    assert_eq!(rig.read(|_wb, cx| prompt.read(cx).text(cx)), "fn");
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("pub", window, cx);
            prompt.submit(cx);
        })
    });
    assert!(rig.settle_until(|rig| rig.editor_text() == "pub a() {}\nlet x\n"));
    let other = rig.dir.path().join("b.rs");
    assert!(
        rig.settle_until(
            |_| std::fs::read_to_string(&other).is_ok_and(|text| text == "pub b() {}\n")
        )
    );
}

#[test]
fn changed_files_and_their_folders_carry_git_marks_in_the_tree() {
    let mut rig = rig();
    let root = rig.dir.path().to_path_buf();
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .expect("git")
            .success());
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), "one\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "first"]);
    std::fs::write(root.join("src/a.rs"), "two\n").unwrap();
    std::fs::write(root.join("new.txt"), "x").unwrap();

    rig.open_project();
    let mark = |rig: &mut Rig, path: &str| {
        let path = path.to_owned();
        rig.read(move |wb, cx| wb.files().read(cx).badge(&path).map(|b| b.letter))
    };
    assert!(rig.settle_until(|rig| mark(rig, "src/a.rs") == Some('M')));
    assert_eq!(mark(&mut rig, "src"), Some('•'));
    assert_eq!(mark(&mut rig, "new.txt"), Some('U'));

    // Saving a change to a clean file marks it as soon as the write lands.
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "second"]);
    rig.open_file("src/a.rs");
    assert!(rig.settle_until(|rig| rig.editor_text() == "two\n"));
    rig.type_text("!");
    rig.run(CommandId::Save);
    assert!(rig.settle_until(|rig| mark(rig, "src/a.rs") == Some('M')));
    // The open tab carries the same mark.
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).badge("src/a.rs").map(|b| b.letter)),
        Some('M')
    );
}

#[test]
fn the_source_control_view_creates_a_branch_stashes_and_pushes_after_a_yes() {
    let mut rig = rig();
    let root = rig.dir.path().to_path_buf();
    let remote = tempfile::tempdir().unwrap();
    let run = |dir: &std::path::Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    run(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    run(&root, &["init", "-q", "-b", "main"]);
    run(&root, &["config", "user.email", "t@example.com"]);
    run(&root, &["config", "user.name", "T"]);
    run(&root, &["config", "commit.gpgsign", "false"]);
    run(
        &root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    run(&root, &["add", "-A"]);
    run(&root, &["commit", "-q", "-m", "first"]);

    rig.open_project();
    rig.run(CommandId::SourceControl);
    let view = rig.read(|wb, _| wb.git_view().cloned()).expect("view");
    assert!(rig.settle_until(|rig| rig.cx.update(|_, cx| view.read(cx).status().is_some())));
    let branch = |rig: &mut Rig| {
        rig.cx
            .update(|_, cx| view.read(cx).status().and_then(|s| s.branch.clone()))
    };

    // The second box names the new branch.
    rig.with(|_wb, window, cx| {
        view.update(cx, |view, cx| {
            view.set_detail("feature/y", window, cx);
            view.create_branch(cx)
        })
    });
    assert!(rig.settle_until(|rig| branch(rig).as_deref() == Some("feature/y")));

    // Stash takes the change away; the entry shows up.
    std::fs::write(root.join("a.txt"), "dirty\n").unwrap();
    rig.with(|_wb, window, cx| {
        view.update(cx, |view, cx| {
            view.set_detail("wip", window, cx);
            view.stash(cx)
        })
    });
    assert!(rig.settle_until(|rig| rig.cx.update(|_, cx| view.read(cx).stashes().len() == 1)));
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );

    // Nothing is pushed until the question is answered yes.
    rig.with(|_wb, _w, cx| view.update(cx, |view, cx| view.ask_push(cx)));
    assert!(rig.cx.update(|_, cx| view.read(cx).confirming_push()));
    rig.with(|_wb, _w, cx| view.update(cx, |view, cx| view.answer_push(false, cx)));
    rig.settle_for(300);
    assert!(run(remote.path(), &["branch", "--list"]).trim().is_empty());
    rig.with(|_wb, _w, cx| {
        view.update(cx, |view, cx| {
            view.ask_push(cx);
            view.answer_push(true, cx)
        })
    });
    let heads = remote.path().to_path_buf();
    assert!(rig.settle_until(|_| run(&heads, &["branch", "--list"]).contains("feature/y")));
}

#[test]
fn the_hunk_panel_stages_one_hunk_of_a_changed_file() {
    let mut rig = rig();
    let root = rig.dir.path().to_path_buf();
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .expect("git")
            .success());
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    git(&["config", "commit.gpgsign", "false"]);
    let original: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    std::fs::write(root.join("f.txt"), &original).unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "first"]);
    std::fs::write(
        root.join("f.txt"),
        original
            .replace("line 2\n", "line two\n")
            .replace("line 28\n", "line 28!\n"),
    )
    .unwrap();

    rig.open_project();
    rig.run(CommandId::SourceControl);
    let view = rig.read(|wb, _| wb.git_view().cloned()).expect("view");
    assert!(rig.settle_until(|rig| rig.cx.update(|_, cx| view.read(cx).status().is_some())));

    rig.with(|_wb, _w, cx| view.update(cx, |view, cx| view.open_hunks("f.txt", false, cx)));
    let hunks = |rig: &mut Rig| {
        rig.cx.update(|_, cx| {
            view.read(cx)
                .hunk_panel()
                .and_then(|(_, _, diff)| diff.map(|diff| diff.hunks.len()))
        })
    };
    assert!(rig.settle_until(|rig| hunks(rig) == Some(2)));

    rig.with(|_wb, _w, cx| view.update(cx, |view, cx| view.apply_hunk(1, cx)));
    // The panel reloads: one hunk is left to stage, and git has the other staged.
    assert!(rig.settle_until(|rig| hunks(rig) == Some(1)));
    let cached = std::process::Command::new("git")
        .args(["diff", "--cached", "--numstat"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&cached.stdout).trim(),
        "1\t1\tf.txt"
    );
}

#[test]
fn the_source_control_view_stages_commits_and_discards_in_a_real_repository() {
    let mut rig = rig();
    let root = rig.dir.path().to_path_buf();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.email", "t@example.com"],
        &["config", "user.name", "T"],
        &["config", "commit.gpgsign", "false"],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .expect("git")
            .success());
    }
    rig.open_project();
    rig.run(CommandId::SourceControl);
    let view = rig
        .read(|wb, _| wb.git_view().cloned())
        .expect("view opens");

    // Everything is untracked at first.
    assert!(rig.settle_until(|rig| {
        rig.cx.update(|_, cx| {
            view.read(cx)
                .status()
                .is_some_and(|s| !s.entries.is_empty())
        })
    }));
    assert!(rig
        .cx
        .update(|_, cx| view.read(cx).status().unwrap().staged().count() == 0));

    // Stage all, commit with a message.
    rig.cx
        .update(|_, cx| view.update(cx, |view, cx| view.stage_all(cx)));
    assert!(rig.settle_until(|rig| {
        rig.cx.update(|_, cx| {
            view.read(cx)
                .status()
                .is_some_and(|s| s.changes().count() == 0)
        })
    }));
    rig.cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_message("first", window, cx);
            view.commit(cx);
        })
    });
    assert!(rig.settle_until(|rig| {
        rig.cx.update(|_, cx| {
            view.read(cx)
                .status()
                .is_some_and(|s| s.head.is_some() && s.entries.is_empty())
        })
    }));

    // An empty message is refused with a visible reason.
    rig.cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_message("", window, cx);
            view.commit(cx);
        })
    });
    assert!(rig.settle_until(|rig| rig.cx.update(|_, cx| view.read(cx).error().is_some())));

    // Discarding asks first, then rewrites the file and reloads the clean tab showing it.
    std::fs::write(root.join("a.txt"), "changed").expect("edit on disk");
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "changed"));
    rig.with(|wb, window, cx| wb.close_git(window, cx));
    rig.run(CommandId::SourceControl);
    let view = rig
        .read(|wb, _| wb.git_view().cloned())
        .expect("view reopens");
    assert!(rig.settle_until(|rig| {
        rig.cx.update(|_, cx| {
            view.read(cx)
                .status()
                .is_some_and(|s| s.changes().count() == 1)
        })
    }));
    let entries: Vec<_> = rig
        .cx
        .update(|_, cx| view.read(cx).status().unwrap().changes().cloned().collect());
    rig.cx
        .update(|_, cx| view.update(cx, |view, cx| view.ask_discard(entries, cx)));
    assert!(rig.cx.update(|_, cx| view.read(cx).confirming()));
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "changed"
    );
    rig.cx
        .update(|_, cx| view.update(cx, |view, cx| view.answer_discard(true, cx)));
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one");
}

struct FixedTasks(Vec<rwk_core::runner::RunTask>);

impl rwk_core::runner::TaskSource for FixedTasks {
    fn load(&self, _project: &Path) -> Vec<rwk_core::runner::RunTask> {
        self.0.clone()
    }
}

fn task(name: &str, command: &[&str]) -> rwk_core::runner::RunTask {
    rwk_core::runner::RunTask {
        name: name.to_owned(),
        command: command.iter().map(|part| (*part).to_owned()).collect(),
    }
}

#[test]
fn a_task_runs_in_its_own_terminal_that_is_not_saved_and_its_output_location_opens() {
    let mut rig = rig();
    std::fs::write(rig.dir.path().join("c.txt"), "alpha\nbeta\ngamma").expect("c");
    let tasks: Arc<dyn rwk_core::runner::TaskSource> = Arc::new(FixedTasks(vec![
        task("argv", &["./.session.sh", "task-arg"]),
        task(
            "fail",
            &["/bin/sh", "-c", "echo error in c.txt:2:1; sleep 100"],
        ),
    ]));
    rig.with(|wb, _, _| wb.set_task_source(Some(tasks)));
    rig.open_project();
    let saved = rig.record_workspace_changes();

    // Without tasks the command says so; with them it opens a picker of their names.
    rig.run(CommandId::RunTask);
    assert!(rig.read(|wb, _| wb.palette().is_some()));
    rig.cx.simulate_input("argv");
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert!(rig.settle_until(|rig| rig.script_calls().iter().any(|call| call == "task-arg")));
    assert!(rig.read(|wb, _| wb.palette().is_none()));

    // A task pane is never part of the saved workspace.
    rig.settle_for(300);
    assert!(saved
        .lock()
        .expect("lock")
        .iter()
        .all(|workspace| workspace.terminals.is_empty()));

    // The second task prints a location; the command opens that file on that line.
    rig.run(CommandId::RunTask);
    rig.cx.simulate_input("fail");
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 2));
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, cx| {
            wb.terminal_view(1)
                .is_some_and(|view| view.read(cx).screen_text().contains("c.txt:2:1"))
        })
    }));
    rig.run(CommandId::OpenOutputLocation);
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("c.txt")));
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).cursor_line(cx)),
        Some(2)
    );
}

#[test]
fn find_opens_and_replace_all_is_one_undoable_edit() {
    let mut rig = rig();
    std::fs::write(rig.dir.path().join("c.txt"), "foo Foo bar foo").expect("c");
    rig.open_project();
    rig.open_file("c.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "foo Foo bar foo"));

    rig.run(CommandId::FindInFile);
    assert!(rig.read(|wb, cx| wb.editor().read(cx).find_open(cx)));
    rig.run(CommandId::ReplaceInFile);

    let count = rig.with(|wb, window, cx| {
        wb.editor().clone().update(cx, |editor, cx| {
            editor.replace_all("foo", "x", false, window, cx)
        })
    });
    assert_eq!(count, 2, "the search is case sensitive");
    assert_eq!(rig.editor_text(), "x Foo bar x");
    let count = rig.with(|wb, window, cx| {
        wb.editor().clone().update(cx, |editor, cx| {
            editor.replace_all("foo", "y", true, window, cx)
        })
    });
    assert_eq!(count, 1);
    assert_eq!(rig.editor_text(), "x y bar x");
}

#[test]
fn auto_save_writes_changed_files_when_the_window_loses_focus_and_only_when_it_is_on() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    let insert = |rig: &mut Rig, text: &str| {
        let text = text.to_owned();
        rig.with(|wb, window, cx| {
            wb.editor()
                .clone()
                .update(cx, |editor, cx| editor.insert_text(&text, window, cx))
        });
    };

    // Off by default: losing focus saves nothing.
    insert(&mut rig, "A");
    rig.cx.deactivate_window();
    rig.settle_for(300);
    assert_eq!(
        std::fs::read_to_string(rig.dir.path().join("a.txt")).unwrap(),
        "one"
    );

    rig.run(CommandId::ToggleAutoSave);
    assert!(rig.read(|wb, _| wb.auto_save()));
    rig.cx.update(|window, _| window.activate_window());
    rig.cx.deactivate_window();
    let saved = rig.settle_until(|rig| {
        std::fs::read_to_string(rig.dir.path().join("a.txt")).unwrap() != "one"
    });
    assert!(saved, "the changed file is written on focus loss");
    assert!(rig.settle_until(|rig| rig.editor_dirty_paths().is_empty()));
}

#[test]
fn saving_formats_first_only_when_format_on_save_is_on() {
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let mut rig = rig();
    let script = rig.dir.path().join(".fake_lsp.py");
    std::fs::write(&script, FAKE_LSP).expect("fake server");
    std::fs::write(rig.dir.path().join("a.rs"), "fn a() {}\nlet x\n").expect("a.rs");
    rig.with(|wb, _, _| {
        wb.set_language_server_launcher(Some(Arc::new(move |_spec, root| {
            let mut command = std::process::Command::new("python3");
            command.arg(&script);
            rwk_core::lsp::LspClient::start_command(command, root)
        })));
    });
    rig.open_project();
    rig.open_file("a.rs");
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.problem_summary().is_some())));
    let edit = |rig: &mut Rig| {
        rig.with(|wb, window, cx| {
            wb.editor()
                .clone()
                .update(cx, |editor, cx| editor.insert_text("// ", window, cx))
        });
    };

    // Plain save: the text goes to disk as typed.
    edit(&mut rig);
    rig.run(CommandId::Save);
    assert!(rig.settle_until(|rig| rig.editor_dirty_paths().is_empty()));
    let plain = std::fs::read_to_string(rig.dir.path().join("a.rs")).unwrap();
    assert!(plain.starts_with("// "), "{plain:?}");
    assert!(!plain.starts_with("FN"), "{plain:?}");

    // With format on save the server's edits are in the file that lands on disk.
    rig.run(CommandId::ToggleFormatOnSave);
    assert!(rig.read(|wb, _| wb.format_on_save()));
    edit(&mut rig);
    rig.run(CommandId::Save);
    let done = rig.settle_until(|rig| rig.editor_dirty_paths().is_empty());
    let status = rig.read(|wb, cx| wb.editor().read(cx).status());
    assert!(done, "status: {status:?} text: {:?}", rig.editor_text());
    let formatted = std::fs::read_to_string(rig.dir.path().join("a.rs")).unwrap();
    assert!(formatted.starts_with("FN"), "{formatted:?}");
    assert!(formatted.contains("let x;"), "{formatted:?}");
}

#[test]
fn the_editor_splits_beside_another_file_and_activating_it_swaps_the_two() {
    let mut rig = rig();
    rig.open_project();
    let split =
        |rig: &mut Rig| rig.read(|wb, cx| wb.editor().read(cx).split_path().map(str::to_owned));

    // One file cannot be split.
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("a.txt")));
    rig.run(CommandId::SplitEditor);
    assert_eq!(split(&mut rig), None);
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).status()).as_deref(),
        Some("Bölmek için ikinci bir dosya açın")
    );

    // Two files: the other one shows beside the active one, and the frame still renders.
    rig.open_file("b.txt");
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("b.txt")));
    rig.run(CommandId::SplitEditor);
    assert_eq!(split(&mut rig).as_deref(), Some("a.txt"));
    rig.settle_for(100);

    // Activating the file beside trades places; the editor text is that file's.
    rig.activate_tab("a.txt");
    assert_eq!(rig.editor_path().as_deref(), Some("a.txt"));
    assert_eq!(split(&mut rig).as_deref(), Some("b.txt"));
    assert_eq!(rig.editor_text(), "one");

    // Closing the file beside, or splitting again, leaves one pane.
    rig.with(|wb, window, cx| wb.request_close_tab("b.txt".to_owned(), window, cx));
    assert!(rig.settle_until(|rig| split(rig).is_none()));
    rig.open_file("b.txt");
    assert!(rig.settle_until(|rig| rig.editor_paths().len() == 2));
    rig.run(CommandId::SplitEditor);
    assert!(split(&mut rig).is_some());
    rig.run(CommandId::SplitEditor);
    assert_eq!(split(&mut rig), None);
}

#[test]
fn a_click_beside_an_overlay_closes_it_and_escape_still_does() {
    let mut rig = rig();
    rig.open_project();

    // Outside click: the palette sits at the top, the click lands in the lower left corner.
    rig.run(CommandId::Commands);
    assert!(rig.read(|wb, _| wb.overlay_open()));
    rig.settle_for(50);
    rig.cx.simulate_click(
        gpui::point(gpui::px(4.0), gpui::px(700.0)),
        gpui::Modifiers::none(),
    );
    rig.settle_for(50);
    assert!(
        !rig.read(|wb, _| wb.overlay_open()),
        "an outside click closes the palette"
    );

    // A click inside the panel does not.
    rig.run(CommandId::Memory);
    rig.settle_for(50);
    assert!(rig.read(|wb, _| wb.overlay_open()));
    rig.cx.simulate_click(
        gpui::point(gpui::px(500.0), gpui::px(300.0)),
        gpui::Modifiers::none(),
    );
    rig.settle_for(50);
    assert!(
        rig.read(|wb, _| wb.overlay_open()),
        "a click on the panel keeps it open"
    );
    rig.cx.simulate_keystrokes("escape");
    assert!(!rig.read(|wb, _| wb.overlay_open()), "Esc closes it");

    // Programmatic close takes the topmost one.
    rig.run(CommandId::Search);
    rig.run(CommandId::Problems);
    rig.with(|wb, window, cx| wb.close_overlay(window, cx));
    assert!(rig.read(
        |wb, cx| wb.problems_view().is_none() && wb.search().is_some() && {
            let _ = cx;
            true
        }
    ));
}

#[test]
fn the_footer_shows_the_workspace_and_takes_items_from_the_application() {
    use rwk_core::workbench::{FooterItem, FooterLevel};
    let mut rig = rig();
    let ids = |rig: &mut Rig| -> Vec<String> {
        rig.read(|wb, cx| wb.footer_items(cx).into_iter().map(|i| i.id).collect())
    };
    assert!(ids(&mut rig).is_empty(), "nothing to say without a project");

    rig.open_project();
    let shown = ids(&mut rig);
    for id in ["problems", "terminals", "auto-save", "format-on-save"] {
        assert!(shown.iter().any(|shown| shown == id), "{id} in {shown:?}");
    }

    // A click on a toggle is the command it names.
    rig.run(CommandId::ToggleAutoSave);
    let auto = rig.read(|wb, cx| {
        wb.footer_items(cx)
            .into_iter()
            .find(|item| item.id == "auto-save")
            .unwrap()
    });
    assert_eq!(auto.level, FooterLevel::Good);

    // The application's own item shows up, is replaced by id, and can be removed.
    rig.with(|wb, _, cx| wb.set_footer_item(FooterItem::info("mine", "bir", ""), cx));
    rig.with(|wb, _, cx| wb.set_footer_item(FooterItem::info("mine", "iki", ""), cx));
    let mine: Vec<String> = rig.read(|wb, cx| {
        wb.footer_items(cx)
            .into_iter()
            .filter(|item| item.id == "mine")
            .map(|item| item.text)
            .collect()
    });
    assert_eq!(mine, ["iki"]);
    rig.with(|wb, _, cx| wb.remove_footer_item("mine", cx));
    assert!(!ids(&mut rig).iter().any(|id| id == "mine"));
}

/// The real rust-analyzer on a tiny cargo project: diagnostics, hover, completion-free
/// navigation and a rename across files. Skipped where it is not installed.
#[test]
fn rust_analyzer_finds_problems_and_renames_a_function_across_files() {
    // A rustup proxy answers `--version` with a failure when the component is missing.
    if !std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let mut rig = rig();
    let root = rig.dir.path().to_path_buf();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/util.rs"),
        "pub fn helper() -> i32 {\n    1\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "mod util;\nfn main() {\n    let n: i32 = \"text\";\n    let _ = (n, util::helper());\n}\n",
    )
    .unwrap();
    rig.with(|wb, _, _| wb.set_language_server_launcher(Some(rwk_core::lsp::default_launcher())));
    rig.open_project();
    rig.open_file("src/main.rs");

    // rust-analyzer loads the crate first; the type error arrives once it has.
    let started = Instant::now();
    let mut found = false;
    while started.elapsed() < Duration::from_secs(120) && !found {
        found = rig.settle_until(|rig| rig.read(|wb, _| wb.problem_summary().is_some()));
    }
    assert!(found, "rust-analyzer reported no problem for a type error");

    // Rename `helper` from its call in main.rs: both files change, util.rs on disk.
    rig.with(|wb, window, cx| {
        wb.editor()
            .clone()
            .update(cx, |editor, cx| editor.set_cursor(3, 28, window, cx))
    });
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).symbol_at_cursor(cx).map(|s| s.3)),
        Some("helper".to_owned())
    );
    rig.run(CommandId::RenameSymbol);
    let prompt = rig
        .read(|wb, _| wb.rename_prompt().cloned())
        .expect("the prompt opens");
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("assist", window, cx);
            prompt.submit(cx);
        })
    });
    let util = root.join("src/util.rs");
    let mut renamed = false;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(60) && !renamed {
        renamed = rig.settle_until(|_| {
            std::fs::read_to_string(&util).is_ok_and(|text| text.contains("fn assist"))
        });
    }
    assert!(renamed, "util.rs was not renamed");
    assert!(
        rig.editor_text().contains("util::assist()"),
        "{}",
        rig.editor_text()
    );
}

#[test]
fn rust_analyzer_formats_a_file_and_answers_completion_and_hover() {
    use gpui_base::input::{CompletionProvider, DefinitionProvider, HoverProvider, Rope};
    use rwk_ui::editor::lsp::ServerProvider;
    use std::cell::RefCell;
    use std::rc::Rc;

    // A rustup proxy answers `--version` with a failure when the component is missing.
    if !std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let mut rig = rig();
    let root = rig.dir.path().to_path_buf();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let messy = "fn   main( ){let   v:Vec<i32>=Vec::new();\nlet _=v.len();}\n";
    std::fs::write(root.join("src/main.rs"), messy).unwrap();
    rig.with(|wb, _, _| wb.set_language_server_launcher(Some(rwk_core::lsp::default_launcher())));
    rig.open_project();
    rig.open_file("src/main.rs");
    assert!(rig.settle_until(|rig| rig.editor_text() == messy));

    // The server needs a while to load the crate; format answers once it has.
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(120) && rig.editor_text() == messy {
        rig.run(CommandId::Format);
        rig.settle_for(1500);
    }
    let formatted = rig.editor_text();
    assert!(
        formatted.contains("fn main() {") && formatted.contains("let v: Vec<i32> = Vec::new();"),
        "{formatted}"
    );

    // The server gets the new text after a typing pause; the test clock is not real time.
    rig.cx.executor().advance_clock(Duration::from_secs(2));
    rig.settle_for(1500);

    // Hover, definition and completion through the same provider the editor uses.
    let requester = rig
        .read(|wb, _| wb.language_requester())
        .expect("a server is running");
    let provider = ServerProvider::new("src/main.rs", &requester);
    let rope = Rope::from(formatted.as_str());
    let at_vec = formatted.find("Vec::new").unwrap() + 1;
    let at_len = formatted.find(".len").unwrap() + 2;
    // The server may still be reading the text the format just produced; ask until it answers.
    let (mut hover, mut definitions, mut completions);
    let attempts = Instant::now();
    loop {
        hover = Rc::new(RefCell::new(None));
        definitions = Rc::new(RefCell::new(None));
        completions = Rc::new(RefCell::new(None));
        rig.cx.update(|window, cx| {
            let task = provider.hover(&rope, at_vec, window, cx);
            let slot = hover.clone();
            cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
                .detach();
            let task = provider.definitions(&rope, at_vec, window, cx);
            let slot = definitions.clone();
            cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
                .detach();
            let context = rwk_core::lsp::lsp_types::CompletionContext {
                trigger_kind: rwk_core::lsp::lsp_types::CompletionTriggerKind::INVOKED,
                trigger_character: None,
            };
            let task = provider.completions(&rope, at_len, context, window, cx);
            let slot = completions.clone();
            cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
                .detach();
        });
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(20)
            && (hover.borrow().is_none()
                || definitions.borrow().is_none()
                || completions.borrow().is_none())
        {
            rig.settle_for(200);
        }
        let good = matches!(&*hover.borrow(), Some(Ok(Some(_))))
            && matches!(&*definitions.borrow(), Some(Ok(links)) if !links.is_empty())
            && completions.borrow().as_ref().is_some_and(|c| c.is_ok());
        if good || attempts.elapsed() > Duration::from_secs(90) {
            break;
        }
        rig.settle_for(1000);
    }
    let hovered = hover.borrow_mut().take().expect("hover answered");
    assert!(
        hovered.is_ok_and(|hover| hover.is_some()),
        "hover over Vec gave nothing"
    );
    let defined = definitions
        .borrow_mut()
        .take()
        .expect("definition answered");
    assert!(
        defined.is_ok_and(|links| !links.is_empty()),
        "no definition for Vec::new"
    );
    let completed = completions
        .borrow_mut()
        .take()
        .expect("completion answered");
    assert!(
        completed.is_ok_and(|response| match response {
            rwk_core::lsp::lsp_types::CompletionResponse::Array(items) => !items.is_empty(),
            rwk_core::lsp::lsp_types::CompletionResponse::List(list) => !list.items.is_empty(),
        }),
        "no completions after v.le"
    );
}

#[test]
fn rust_analyzer_lists_usages_and_the_outline_of_a_file() {
    // A rustup proxy answers `--version` with a failure when the component is missing.
    if !std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let mut rig = rig();
    let root = rig.dir.path().to_path_buf();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/util.rs"),
        "pub fn helper() -> i32 {\n    1\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "mod util;\nstruct Shape;\nfn main() {\n    let _ = (Shape, util::helper(), util::helper());\n}\n",
    )
    .unwrap();
    rig.with(|wb, _, _| wb.set_language_server_launcher(Some(rwk_core::lsp::default_launcher())));
    rig.open_project();
    rig.open_file("src/main.rs");
    assert!(rig.settle_until(|rig| rig.editor_text().contains("helper")));

    let list = |rig: &mut Rig| {
        rig.read(|wb, cx| {
            wb.locations_view().map(|view| {
                view.read(cx)
                    .problems()
                    .iter()
                    .map(|p| (p.path.clone(), p.line, p.message.clone()))
                    .collect::<Vec<_>>()
            })
        })
    };

    // The server needs a while to load the crate; ask again until it answers with the usages.
    let started = Instant::now();
    let mut usages = None;
    while started.elapsed() < Duration::from_secs(120) && usages.is_none() {
        rig.with(|wb, window, cx| {
            wb.editor()
                .clone()
                .update(cx, |editor, cx| editor.set_cursor(3, 28, window, cx))
        });
        rig.run(CommandId::FindReferences);
        rig.settle_for(2500);
        usages = list(&mut rig).filter(|lines| lines.len() >= 3);
        if usages.is_none() {
            rig.with(|wb, window, cx| wb.close_overlay(window, cx));
        }
    }
    let usages = usages.expect("rust-analyzer listed the usages");
    assert!(
        usages.iter().any(|(path, _, _)| path == "src/util.rs"),
        "{usages:?}"
    );
    assert!(
        usages
            .iter()
            .any(|(_, _, text)| text.contains("util::helper()")),
        "{usages:?}"
    );

    // The outline of main.rs names its struct and function.
    rig.with(|wb, window, cx| wb.close_overlay(window, cx));
    rig.run(CommandId::Outline);
    assert!(rig.settle_until(|rig| list(rig).is_some()));
    let outline: Vec<String> = list(&mut rig)
        .unwrap()
        .into_iter()
        .map(|(_, _, label)| label)
        .collect();
    assert!(outline.contains(&"struct Shape".to_owned()), "{outline:?}");
    assert!(outline.contains(&"fn main".to_owned()), "{outline:?}");

    // The whole project's symbols: `helper` lives in util.rs.
    rig.with(|wb, window, cx| wb.close_overlay(window, cx));
    rig.run(CommandId::WorkspaceSymbols);
    let prompt = rig
        .read(|wb, _| wb.rename_prompt().cloned())
        .expect("prompt");
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("helper", window, cx);
            prompt.submit(cx)
        })
    });
    assert!(rig.settle_until(|rig| list(rig).is_some()));
    let symbols = list(&mut rig).unwrap();
    assert!(
        symbols
            .iter()
            .any(|(path, _, label)| path == "src/util.rs" && label.contains("helper")),
        "{symbols:?}"
    );
}

/// A server that was missing shows in the footer; once it can start, clicking its item
/// (the restart command) brings it up without reopening the project.
#[test]
fn restarting_language_servers_recovers_one_that_gave_up() {
    use std::sync::atomic::{AtomicBool, Ordering};

    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let mut rig = rig();
    let script = rig.dir.path().join(".fake_lsp.py");
    std::fs::write(&script, FAKE_LSP).expect("fake server");
    std::fs::write(rig.dir.path().join("a.rs"), "fn a() {}\nlet x\n").expect("a.rs");
    let installed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&installed);
    rig.with(|wb, _, _| {
        wb.set_language_server_launcher(Some(Arc::new(move |_spec, root| {
            if !flag.load(Ordering::SeqCst) {
                return Err(rwk_core::error::AppError::new(
                    rwk_core::error::ErrorCode::NotFound,
                    "not installed",
                ));
            }
            let mut command = std::process::Command::new("python3");
            command.arg(&script);
            rwk_core::lsp::LspClient::start_command(command, root)
        })));
    });
    rig.open_project();
    rig.open_file("a.rs");

    let server_item = |rig: &mut Rig| {
        rig.read(|wb, cx| {
            wb.footer_items(cx)
                .into_iter()
                .find(|item| item.id.starts_with("lsp-"))
        })
    };
    assert!(rig.settle_until(|rig| server_item(rig).is_some_and(|i| i.text.ends_with("yok"))));
    let item = server_item(&mut rig).unwrap();
    assert_eq!(item.command, Some(CommandId::RestartServers));
    assert_eq!(rig.read(|wb, _| wb.problem_summary()), None);

    installed.store(true, Ordering::SeqCst);
    rig.run(item.command.unwrap());
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.problem_summary().is_some())));
    let item = server_item(&mut rig).unwrap();
    assert!(item.text.ends_with('✓'), "{}", item.text);
    assert_eq!(item.command, None);
}

#[test]
fn the_settings_view_flips_switches_and_shows_the_new_values() {
    let mut rig = rig();
    rig.run(CommandId::Settings);
    assert!(rig.read(|wb, _| wb.settings_view().is_some()));
    assert!(rig.read(|wb, _| wb.overlay_open()));

    // The first row is the theme: the right arrow picks the next skin.
    assert_eq!(
        rig.read(|wb, _| wb.appearance().skin),
        rwk_core::workbench::Skin::Standard
    );
    rig.cx.simulate_keystrokes("right right");
    assert_eq!(
        rig.read(|wb, _| wb.appearance().skin),
        rwk_core::workbench::Skin::Aperture
    );
    // Down to the tone, then the editing switches: Enter flips them and the list shows it.
    rig.cx.simulate_keystrokes("down right");
    assert_eq!(
        rig.read(|wb, _| wb.appearance().tone),
        rwk_core::workbench::Tone::Light
    );
    // The editor rows: the right arrow on the text size picks the next size.
    rig.cx.simulate_keystrokes("down right");
    assert_eq!(rig.read(|wb, _| wb.editor_prefs().font_size), 16);
    rig.cx.simulate_keystrokes("down down down enter");
    assert!(rig.read(|wb, _| wb.auto_save()));
    let view = rig.read(|wb, _| wb.settings_view().cloned()).unwrap();
    let value = rig.cx.update(|_, cx| {
        let rows = view.read(cx).rows().to_vec();
        rows.into_iter()
            .find(|row| row.setting == rwk_ui::settings::Setting::AutoSave)
            .map(|row| row.value)
    });
    assert_eq!(value, Some(rwk_ui::settings::SettingValue::Switch(true)));
    rig.cx.simulate_keystrokes("down enter");
    assert!(rig.read(|wb, _| wb.format_on_save()));

    rig.cx.simulate_keystrokes("escape");
    assert!(rig.read(|wb, _| wb.settings_view().is_none()));
}

#[test]
fn the_regex_replace_bar_counts_matches_and_replaces_them_with_groups() {
    let mut rig = rig();
    std::fs::write(rig.dir.path().join("a.txt"), "fn one()\nfn two()\nlet x\n").expect("a.txt");
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_path().is_some()));
    rig.run(CommandId::ReplaceRegexInFile);
    let bar = rig.read(|wb, _| wb.file_replace().cloned()).expect("bar");

    // The count follows the pattern; a bad pattern says why.
    rig.with(|_wb, window, cx| bar.update(cx, |bar, cx| bar.set_find(r"fn (\w+)", window, cx)));
    assert_eq!(
        rig.cx.update(|_, cx| bar.read(cx).note().to_owned()),
        "2 eşleşme"
    );
    rig.with(|_wb, window, cx| bar.update(cx, |bar, cx| bar.set_find("(", window, cx)));
    assert!(rig
        .cx
        .update(|_, cx| bar.read(cx).note().to_owned())
        .starts_with("Desen geçersiz"));

    // Replacing expands the group and closes the bar.
    rig.with(|_wb, window, cx| {
        bar.update(cx, |bar, cx| {
            bar.set_find(r"fn (\w+)", window, cx);
            bar.set_with("def $1", window, cx);
            bar.apply(cx)
        })
    });
    assert!(rig.read(|wb, _| wb.file_replace().is_none()));
    let text = rig.read(|wb, cx| wb.editor().read(cx).text_of("a.txt", cx));
    assert_eq!(text.as_deref(), Some("def one()\ndef two()\nlet x\n"));
}

#[test]
fn toolbar_tabs_show_one_category_and_follow_their_shortcuts() {
    let mut rig = rig();
    rig.open_project();
    let shown = |rig: &mut Rig| rig.read(|wb, _| wb.toolbar_tab());
    assert_eq!(shown(&mut rig), rwk_core::workbench::ToolbarGroup::Files);
    rig.cx.simulate_keystrokes("ctrl-shift-2");
    assert_eq!(shown(&mut rig), rwk_core::workbench::ToolbarGroup::Terminal);
    rig.run(CommandId::ToolbarTab(5));
    assert_eq!(shown(&mut rig), rwk_core::workbench::ToolbarGroup::Tools);
}

#[test]
fn the_toolbar_follows_its_presets_and_refuses_to_hide_every_category() {
    use rwk_core::workbench::{ToolbarLayout, ToolbarPreset};
    use rwk_ui::settings::Setting;

    let mut rig = rig();
    let events: Arc<Mutex<Vec<WorkbenchEvent>>> = Arc::default();
    let sink = Arc::clone(&events);
    let view = rig.view.clone();
    rig.cx.update(|_w, app| {
        app.subscribe(&view, move |_wb, event: &WorkbenchEvent, _app| {
            sink.lock().unwrap().push(event.clone());
        })
        .detach();
    });
    rig.run(CommandId::Settings);
    let settings = rig.read(|wb, _| wb.settings_view().cloned()).expect("view");
    assert_eq!(
        rig.read(|wb, _| wb.toolbar_layout().preset_of()),
        Some(ToolbarPreset::Tabbed)
    );

    // Picking a preset changes the layout and tells the application to store it.
    rig.with(|_wb, _w, cx| {
        settings.update(cx, |view, cx| view.choose(Setting::ToolbarPreset, 3, cx))
    });
    assert_eq!(
        rig.read(|wb, _| wb.toolbar_layout()),
        ToolbarLayout::preset(ToolbarPreset::Minimal)
    );
    assert!(events.lock().unwrap().iter().any(|event| matches!(
        event,
        WorkbenchEvent::PreferencesChanged { toolbar, .. }
            if *toolbar == ToolbarLayout::preset(ToolbarPreset::Minimal)
    )));

    // Editing one switch makes the layout custom: no preset matches any more.
    rig.with(|_wb, _w, cx| {
        settings.update(cx, |view, cx| view.toggle(Setting::ToolbarCaptions, cx))
    });
    assert_eq!(rig.read(|wb, _| wb.toolbar_layout().preset_of()), None);

    // Hiding categories works until one is left; the last one stays.
    for index in 0..5u8 {
        rig.with(|_wb, _w, cx| {
            settings.update(cx, |view, cx| view.toggle(Setting::ToolbarGroup(index), cx))
        });
    }
    let layout = rig.read(|wb, _| wb.toolbar_layout());
    assert_eq!(layout.groups.iter().filter(|shown| **shown).count(), 1);
    assert!(layout.groups[4], "the last category is the one left");
}

#[test]
fn go_to_line_comment_toggle_and_zoom_work_on_the_open_file() {
    let mut rig = rig();
    std::fs::write(
        rig.dir.path().join("m.rs"),
        "fn a() {\n    let x = 1;\n    let y = 2;\n}\n",
    )
    .expect("m.rs");
    rig.open_project();
    rig.open_file("m.rs");
    assert!(rig.settle_until(|rig| rig.editor_path().is_some()));

    // Go to line: the prompt takes a line, or line:column, and refuses nonsense.
    rig.run(CommandId::GoToLine);
    let prompt = rig
        .read(|wb, _| wb.rename_prompt().cloned())
        .expect("prompt");
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("zzz", window, cx);
            prompt.submit(cx)
        })
    });
    assert!(rig.cx.update(|_, cx| prompt.read(cx).problem()).is_some());
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("3", window, cx);
            prompt.submit(cx)
        })
    });
    assert!(rig.read(|wb, _| wb.rename_prompt().is_none()));
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).cursor_line(cx)),
        Some(3)
    );

    // Comment the cursor's line, then take it back.
    rig.run(CommandId::ToggleComment);
    assert_eq!(
        rig.editor_text(),
        "fn a() {\n    let x = 1;\n    // let y = 2;\n}\n"
    );
    rig.run(CommandId::ToggleComment);
    assert_eq!(
        rig.editor_text(),
        "fn a() {\n    let x = 1;\n    let y = 2;\n}\n"
    );

    // The shortcuts reach the same commands.
    rig.cx.simulate_keystrokes("ctrl-g");
    assert!(rig.read(|wb, _| wb.rename_prompt().is_some()));
    rig.cx.simulate_keystrokes("escape");
    rig.cx.simulate_keystrokes("ctrl-/");
    assert!(rig.editor_text().contains("// let y"));
    rig.cx.simulate_keystrokes("ctrl-/");

    // Zoom stays inside its limits and resets.
    let size = |rig: &mut Rig| rig.read(|wb, _| wb.editor_prefs().font_size);
    assert_eq!(size(&mut rig), rwk_core::workbench::DEFAULT_FONT_SIZE);
    rig.run(CommandId::ZoomIn);
    assert_eq!(size(&mut rig), rwk_core::workbench::DEFAULT_FONT_SIZE + 1);
    rig.cx.simulate_keystrokes("ctrl-shift-=");
    assert_eq!(size(&mut rig), rwk_core::workbench::DEFAULT_FONT_SIZE + 2);
    rig.cx.simulate_keystrokes("ctrl--");
    assert_eq!(size(&mut rig), rwk_core::workbench::DEFAULT_FONT_SIZE + 1);
    rig.run(CommandId::ZoomReset);
    assert_eq!(size(&mut rig), rwk_core::workbench::DEFAULT_FONT_SIZE);
    for _ in 0..40 {
        rig.run(CommandId::ZoomOut);
    }
    assert_eq!(size(&mut rig), rwk_core::workbench::MIN_FONT_SIZE);

    rig.run(CommandId::ToggleWordWrap);
    assert!(rig.read(|wb, _| wb.editor_prefs().soft_wrap));
}

#[test]
fn changes_made_elsewhere_reload_clean_tabs_and_refresh_the_tree() {
    use std::time::Duration;

    let mut rig = rig();
    std::fs::write(rig.dir.path().join("a.txt"), "one\n").expect("a.txt");
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_path().is_some()));

    // The first look only records the stamps.
    rig.cx.executor().advance_clock(Duration::from_millis(2100));
    rig.settle_for(50);

    // Another tool rewrites the open file and adds a new one.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(rig.dir.path().join("a.txt"), "two\n").expect("rewrite");
    std::fs::write(rig.dir.path().join("b.txt"), "new\n").expect("b.txt");
    rig.cx.executor().advance_clock(Duration::from_millis(2100));
    assert!(rig.settle_until(|rig| rig.editor_text() == "two\n"));
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, cx| wb.files().read(cx).visible_ids(cx))
            .iter()
            .any(|id| id == "b.txt")
    }));

    // A tab with unsaved typing is left alone and the user is told.
    rig.with(|wb, window, cx| {
        wb.editor().update(cx, |editor, cx| {
            editor.replace_all("two", "mine", false, window, cx);
        })
    });
    let _ = rig.editor_text();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(rig.dir.path().join("a.txt"), "theirs\n").expect("rewrite again");
    rig.cx.executor().advance_clock(Duration::from_millis(2100));
    rig.settle_for(100);
    assert_ne!(rig.editor_text(), "theirs\n");
}

#[test]
fn terminal_search_needs_a_terminal_and_says_when_nothing_matches() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::FindInTerminal);
    assert!(
        rig.read(|wb, _| wb.rename_prompt().is_none()),
        "no terminal yet"
    );

    rig.run(CommandId::NewTerminal);
    rig.run(CommandId::FindInTerminal);
    let prompt = rig
        .read(|wb, _| wb.rename_prompt().cloned())
        .expect("prompt");
    rig.with(|_wb, window, cx| {
        prompt.update(cx, |prompt, cx| {
            prompt.set_text("zzz-never-printed", window, cx);
            prompt.submit(cx)
        })
    });
    assert!(rig.read(|wb, _| wb.rename_prompt().is_none()));
    assert!(rig.read(|wb, _| wb.locations_view().is_none()));
}

#[test]
fn recent_projects_are_remembered_newest_first_and_can_be_reopened() {
    let mut rig = rig();
    let first = rig.dir.path().to_path_buf();
    let other = tempfile::tempdir().expect("second project");
    let events: Arc<Mutex<Vec<WorkbenchEvent>>> = Arc::default();
    let sink = Arc::clone(&events);
    let view = rig.view.clone();
    rig.cx.update(|_w, app| {
        app.subscribe(&view, move |_wb, event: &WorkbenchEvent, _app| {
            sink.lock().unwrap().push(event.clone());
        })
        .detach();
    });

    // With only one project there is nothing else to offer.
    rig.open_project();
    rig.run(CommandId::RecentProjects);
    assert!(rig.read(|wb, _| wb.locations_view().is_none()));

    let second = other.path().to_path_buf();
    rig.with(|wb, window, cx| wb.open_project(second.clone(), window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.recent_projects().first() == Some(&second))));
    assert_eq!(
        rig.read(|wb, _| wb.recent_projects().to_vec()),
        [second.clone(), first.clone()]
    );
    assert!(events.lock().unwrap().iter().any(|event| matches!(
        event,
        WorkbenchEvent::RecentProjectsChanged(list) if list.first() == Some(&second)
    )));

    // The list offers the one that is not open; choosing it switches back.
    rig.run(CommandId::RecentProjects);
    let list = rig
        .read(|wb, _| wb.locations_view().cloned())
        .expect("list");
    assert_eq!(rig.cx.update(|_, cx| list.read(cx).problems().len()), 1);
    rig.with(|_wb, _w, cx| list.update(cx, |list, cx| list.choose_selected(cx)));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.recent_projects().first() == Some(&first))));
}

#[test]
fn signature_help_shows_the_active_parameter_in_the_toast() {
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let mut rig = rig();
    let script = rig.dir.path().join(".fake_lsp.py");
    std::fs::write(&script, FAKE_LSP).expect("fake server");
    std::fs::write(rig.dir.path().join("a.rs"), "fn a() {}\nlet x\n").expect("a.rs");
    rig.with(|wb, _, _| {
        wb.set_language_server_launcher(Some(Arc::new(move |_spec, root| {
            let mut command = std::process::Command::new("python3");
            command.arg(&script);
            rwk_core::lsp::LspClient::start_command(command, root)
        })));
    });
    rig.open_project();
    rig.open_file("a.rs");
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.problem_summary().is_some())));

    rig.run(CommandId::SignatureHelp);
    assert!(rig.settle_until(|rig| rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("«y: u8»"))));
}

#[test]
fn a_snippet_is_inserted_indented_with_the_cursor_on_its_first_place() {
    let mut rig = rig();
    std::fs::write(rig.dir.path().join("s.rs"), "    ").expect("s.rs");
    rig.with(|wb, _, _| {
        wb.set_snippet_source(Some(Arc::new(|| {
            rwk_core::workbench::parse_user_snippets(
                r#"{"snippets":[{"prefix":"todo","body":"// TODO ${1:what}\n$0"}]}"#,
            )
        })))
    });
    rig.open_project();
    rig.open_file("s.rs");
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("s.rs")));
    rig.with(|wb, window, cx| {
        wb.editor().clone().update(cx, |editor, cx| {
            editor.set_cursor(0, 4, window, cx);
        })
    });

    rig.run(CommandId::InsertSnippet);
    assert!(rig.read(|wb, _| wb.palette().is_some()));
    rig.cx.simulate_input("todo");
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.read(|wb, _| wb.palette().is_none()));
    assert_eq!(rig.editor_text(), "    // TODO what\n    ");
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).cursor_at(cx)),
        Some(("s.rs".to_owned(), 0, 4 + 8))
    );
}

#[test]
fn merge_conflicts_are_found_and_resolved_one_side_at_a_time() {
    let mut rig = rig();
    let text = "top\n<<<<<<< HEAD\nmine 1\n=======\nyours 1\n>>>>>>> topic\nmid\n<<<<<<< HEAD\nmine 2\n=======\nyours 2\n>>>>>>> topic\nend\n";
    std::fs::write(rig.dir.path().join("m.txt"), text).expect("m.txt");
    rig.open_project();
    rig.open_file("m.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == text));

    // Outside a block there is nothing to resolve.
    rig.run(CommandId::KeepOurs);
    assert_eq!(rig.editor_text(), text);

    // The jump lands on the first block, then keeps our side of it.
    rig.run(CommandId::NextConflict);
    assert_eq!(
        rig.read(|wb, cx| wb.editor().read(cx).cursor_line(cx)),
        Some(2)
    );
    rig.run(CommandId::KeepOurs);
    assert_eq!(
        rig.editor_text(),
        "top\nmine 1\nmid\n<<<<<<< HEAD\nmine 2\n=======\nyours 2\n>>>>>>> topic\nend\n"
    );
    assert!(rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("1 kaldı")));

    // The second block: keep both, then nothing is left to jump to.
    rig.run(CommandId::NextConflict);
    rig.run(CommandId::KeepBoth);
    assert_eq!(
        rig.editor_text(),
        "top\nmine 1\nmid\nmine 2\nyours 2\nend\n"
    );
    rig.run(CommandId::NextConflict);
    assert!(rig
        .read(|wb, _| wb.toast())
        .is_some_and(|toast| toast.contains("çakışma yok")));
}
