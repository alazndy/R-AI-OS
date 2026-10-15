//! The workbench on GPUI's test platform: real project, real editor, real PTY sessions.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, TestDispatcher, VisualTestContext};
use gpui_wgpu::CosmicTextSystem;
use rwk_core::search::{SearchHit, SearchResult, SearchSource};
use rwk_core::terminal::ProgramPaths;
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
fn the_fifth_terminal_is_refused_with_a_reason() {
    let mut rig = rig();
    rig.open_project();
    for _ in 0..4 {
        rig.run(CommandId::NewTerminal);
    }
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 4);
    rig.run(CommandId::NewTerminal);
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 4);
    assert_eq!(
        rig.read(|wb, _| wb.toast()).as_deref(),
        Some("En fazla 4 terminal açılabilir")
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
        palette.update(cx, |palette, cx| palette.set_query("kaydet", window, cx))
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
use rwk_core::sessions::SessionKind;

fn connected(rig: &mut Rig, policy: PolicyAction) -> Arc<FakeControlPlane> {
    let fake = Arc::new(FakeControlPlane::new());
    fake.set_policy("launch_agent", policy);
    fake.set_policy("create_handoff", policy);
    fake.set_policy("register_project", policy);
    fake.set_policy("approve_handoff", policy);
    fake.set_policy("reject_handoff", policy);
    // The first inspector poll finds the daemon healthy; only then are agent commands offered.
    fake.push_work_snapshot(Ok(raios_contracts::WorkSnapshot::default()))
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
        terminals: vec![SavedTerminal::Shell],
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
        terminals: vec![SavedTerminal::Agent {
            kind: SessionKind::Claude,
            native_session: None,
        }],
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
        terminals: vec![SavedTerminal::Agent {
            kind: SessionKind::Codex,
            native_session: None,
        }],
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
        terminals: vec![SavedTerminal::Agent {
            kind: SessionKind::Codex,
            native_session: None,
        }],
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
        terminals: Vec::new(),
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
    assert_eq!(last.terminals, vec![SavedTerminal::Shell]);
    assert_eq!(last.tabs.len(), 1, "the open tab is part of the state");
}
