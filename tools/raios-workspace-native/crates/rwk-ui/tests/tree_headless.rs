//! The file tree on GPUI's test platform with a real temporary project.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, TestDispatcher, VisualTestContext};
use gpui_wgpu::CosmicTextSystem;
use rwk_core::project::ProjectRoot;
use rwk_ui::file_tree::view::{FileTreeEvent, FileTreeView};

const WAIT: Duration = Duration::from_secs(10);

static SERIAL: Mutex<()> = Mutex::new(());

struct Rig {
    _serial: std::sync::MutexGuard<'static, ()>,
    cx: &'static mut VisualTestContext,
    view: Entity<FileTreeView>,
    events: Arc<Mutex<Vec<FileTreeEvent>>>,
}

fn rig() -> Rig {
    let serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let app = Box::leak(Box::new(TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        Arc::new(CosmicTextSystem::new("monospace")),
    )));
    app.update(gpui_base::init);
    let (view, cx) = app.add_window_view(|_window, cx| FileTreeView::new(cx));
    let events: Arc<Mutex<Vec<FileTreeEvent>>> = Arc::default();
    let sink = Arc::clone(&events);
    cx.update(|_window, app| {
        app.subscribe(&view, move |_view, event: &FileTreeEvent, _app| {
            sink.lock().unwrap().push(event.clone());
        })
        .detach();
    });
    Rig {
        _serial: serial,
        cx,
        view,
        events,
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    project: Arc<ProjectRoot>,
}

fn fixture(files: &[&str], dirs: &[&str]) -> Fixture {
    let dir = tempfile::tempdir().expect("temp dir");
    for path in dirs {
        std::fs::create_dir_all(dir.path().join(path)).expect("directory");
    }
    for path in files {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("directories");
        }
        std::fs::write(full, "x").expect("file");
    }
    let project = Arc::new(ProjectRoot::open(dir.path().to_path_buf()).expect("project"));
    Fixture { dir, project }
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

    fn set_project(&mut self, fixture: &Fixture) {
        let (view, project) = (self.view.clone(), fixture.project.clone());
        self.cx
            .update(|_w, cx| view.update(cx, |tree, cx| tree.set_project(project, cx)));
    }

    fn ids(&mut self) -> Vec<String> {
        let view = self.view.clone();
        self.cx.update(|_w, cx| view.read(cx).visible_ids(cx))
    }

    fn expand(&mut self, path: &str) {
        let (view, path) = (self.view.clone(), path.to_owned());
        self.cx
            .update(|_w, cx| view.update(cx, |tree, cx| tree.expand(&path, cx)));
    }
}

#[test]
fn the_root_lists_directories_first_then_files_case_insensitively() {
    let fixture = fixture(&["b.txt", "A.md"], &["src", "Docs"]);
    let mut rig = rig();
    rig.set_project(&fixture);
    assert!(rig.settle_until(|rig| rig.ids().len() == 4));
    assert_eq!(rig.ids(), ["Docs", "src", "A.md", "b.txt"]);
}

#[test]
fn expanding_a_directory_lists_its_children_lazily() {
    let fixture = fixture(&["src/lib.rs", "src/bin/main.rs"], &[]);
    let mut rig = rig();
    rig.set_project(&fixture);
    assert!(rig.settle_until(|rig| rig.ids().iter().any(|id| id == "src")));
    assert!(!rig.ids().iter().any(|id| id == "src/lib.rs"));
    rig.expand("src");
    assert!(rig.settle_until(|rig| rig.ids().iter().any(|id| id == "src/lib.rs")));
    assert!(rig.ids().iter().any(|id| id == "src/bin"));
    // The nested directory is still unlisted.
    assert!(!rig.ids().iter().any(|id| id == "src/bin/main.rs"));
}

#[test]
fn a_symlink_that_leaves_the_project_is_not_listed_or_opened() {
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("secret.txt"), "x").expect("secret");
    let fixture = fixture(&["inside.txt"], &[]);
    std::os::unix::fs::symlink(outside.path(), fixture.dir.path().join("escape")).expect("link");
    let mut rig = rig();
    rig.set_project(&fixture);
    assert!(rig.settle_until(|rig| !rig.ids().is_empty()));
    assert_eq!(rig.ids(), ["inside.txt"]);
    // Even asking for the link by name opens nothing.
    let view = rig.view.clone();
    rig.cx
        .update(|_w, cx| view.update(cx, |tree, cx| tree.activate("escape", cx)));
    assert!(rig.events.lock().unwrap().is_empty());
}

#[test]
fn activating_a_file_emits_open_file_and_a_folder_does_not() {
    let fixture = fixture(&["src/lib.rs", "a.txt"], &[]);
    let mut rig = rig();
    rig.set_project(&fixture);
    assert!(rig.settle_until(|rig| rig.ids().len() == 2));
    let view = rig.view.clone();
    rig.cx
        .update(|_w, cx| view.update(cx, |tree, cx| tree.activate("src", cx)));
    assert!(rig.events.lock().unwrap().is_empty(), "a folder was opened");
    rig.cx
        .update(|_w, cx| view.update(cx, |tree, cx| tree.activate("a.txt", cx)));
    assert_eq!(
        *rig.events.lock().unwrap(),
        vec![FileTreeEvent::OpenFile("a.txt".to_owned())]
    );
}

#[test]
fn a_listing_from_the_previous_project_is_dropped() {
    let first = fixture(&["from-first.txt"], &[]);
    let second = fixture(&["from-second.txt"], &[]);
    let mut rig = rig();
    rig.set_project(&first);
    // Switch before the first listing could be applied.
    rig.set_project(&second);
    assert!(rig.settle_until(|rig| !rig.ids().is_empty()));
    rig.settle_until(|_| false);
    assert_eq!(rig.ids(), ["from-second.txt"]);
}

#[test]
fn refresh_keeps_expanded_folders_and_the_selection() {
    let fixture = fixture(&["src/lib.rs", "a.txt"], &[]);
    let mut rig = rig();
    rig.set_project(&fixture);
    assert!(rig.settle_until(|rig| rig.ids().len() == 2));
    rig.expand("src");
    assert!(rig.settle_until(|rig| rig.ids().iter().any(|id| id == "src/lib.rs")));
    let view = rig.view.clone();
    rig.cx
        .update(|_w, cx| view.update(cx, |tree, cx| tree.select("src/lib.rs", cx)));
    std::fs::write(fixture.dir.path().join("src/new.rs"), "y").expect("new file");
    rig.cx
        .update(|_w, cx| view.update(cx, |tree, cx| tree.refresh(cx)));
    assert!(rig.settle_until(|rig| rig.ids().iter().any(|id| id == "src/new.rs")));
    assert!(rig.ids().iter().any(|id| id == "src/lib.rs"));
    let selected = rig.cx.update(|_w, cx| view.read(cx).selected_path(cx));
    assert_eq!(selected.as_deref(), Some("src/lib.rs"));
}
