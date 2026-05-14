use crate::app::Editor;
use std::path::{Path, PathBuf};
use std::thread;

use crate::app::{state::*, App};
use crate::compliance;
use crate::filebrowser::{load_file_content, save_file_content, FileEntry};

impl App {
    pub(crate) fn open_file_view(&mut self, entry: FileEntry) {
        let content = load_file_content(&entry.path);
        self.health.compliance = Some(compliance::check_file(&entry.path, &content));
        self.editor.lines = content.lines().map(str::to_owned).collect();
        self.editor.scroll = 0;
        self.editor.watched_mtime = std::fs::metadata(&entry.path)
            .ok()
            .and_then(|m| m.modified().ok());
        self.editor.changed_externally = false;
        self.editor.active_file = Some(entry);
        self.editor.save_msg = None;
        self.state = AppState::FileView;
    }

    pub(crate) fn open_file_edit(&mut self, entry: FileEntry) {
        let content = load_file_content(&entry.path);
        self.health.compliance = Some(compliance::check_file(&entry.path, &content));
        let view_h = self.height.saturating_sub(8) as usize;
        self.editor.editor = Editor::from_content(&content, view_h.max(5));
        self.editor.watched_mtime = std::fs::metadata(&entry.path)
            .ok()
            .and_then(|m| m.modified().ok());
        self.editor.changed_externally = false;
        self.editor.active_file = Some(entry);
        self.editor.save_msg = None;
        self.state = AppState::FileEdit;
    }

    pub(crate) fn open_graph_report(&mut self, project_path: &Path) {
        let report_path = project_path.join("GRAPH_REPORT.md");
        if report_path.exists() {
            let content = load_file_content(&report_path);
            self.projects.graph_report_lines = content.lines().map(str::to_owned).collect();
            self.projects.graph_report_scroll = 0;
            self.state = AppState::GraphReport;
        } else {
            self.system.sync_status = Some("Graph report not found. Run Graphify first.".into());
        }
    }

    pub(crate) fn open_git_diff(&mut self, project_path: &Path) {
        let output = std::process::Command::new("git")
            .current_dir(project_path)
            .args(["diff"])
            .output();

        if let Ok(out) = output {
            let diff = String::from_utf8_lossy(&out.stdout).to_string();
            if diff.trim().is_empty() {
                self.projects.git_diff_lines = vec!["No unstaged changes.".to_string()];
            } else {
                self.projects.git_diff_lines = diff.lines().map(|s| s.to_string()).collect();
            }
            self.projects.git_diff_scroll = 0;
            self.state = AppState::GitDiffView;
        } else {
            self.projects.git_diff_lines = vec!["Failed to run git diff.".to_string()];
            self.state = AppState::GitDiffView;
        }
    }

    pub(crate) fn save_file(&mut self) {
        if let Some(ref file) = self.editor.active_file.clone() {
            let content = self.editor.editor.to_string();
            match save_file_content(&file.path, &content) {
                Ok(()) => {
                    self.editor.lines = content.lines().map(str::to_owned).collect();
                    self.editor.save_msg = Some("Saved!".into());
                    self.state = AppState::FileView;
                }
                Err(e) => {
                    self.editor.save_msg = Some(format!("Error: {}", e));
                }
            }
        }
    }

    pub(crate) fn open_project_detail(&mut self, project: crate::entities::EntityProject) {
        self.projects.memory_scroll = 0;
        self.projects.panel_focus = false;
        self.projects.memory_lines = Vec::new();
        self.projects.git_log = Vec::new();
        self.projects.active = Some(project.clone());
        self.state = AppState::ProjectDetail;

        let tx = self.tx.clone();
        let proj_path = project.local_path.clone();
        thread::spawn(move || {
            let memory_path = proj_path.join("memory.md");
            let content = load_file_content(&memory_path);
            let memory: Vec<String> = content.lines().map(str::to_owned).collect();
            let git_log = crate::filebrowser::get_git_log(&proj_path);
            tx.send(BgMsg::ProjectOpened { memory, git_log }).ok();
        });
    }

    pub(crate) fn wizard_commit_input(&mut self) {
        use crate::setup_wizard::WizardStep;
        match self.wizard.step {
            WizardStep::Workspace => match self.wizard.field_cursor {
                0 => self.wizard.dev_ops = self.wizard.input.clone(),
                1 => self.wizard.github = self.wizard.input.clone(),
                2 => self.wizard.vault = self.wizard.input.clone(),
                _ => {}
            },
            WizardStep::Master => {
                self.wizard.master = self.wizard.input.clone();
            }
            _ => {}
        }
        self.wizard.input.clear();
    }

    pub(crate) fn wizard_start_edit(&mut self) {
        use crate::setup_wizard::WizardStep;
        self.wizard.input = match self.wizard.step {
            WizardStep::Workspace => match self.wizard.field_cursor {
                0 => self.wizard.dev_ops.clone(),
                1 => self.wizard.github.clone(),
                2 => self.wizard.vault.clone(),
                _ => String::new(),
            },
            WizardStep::Master => self.wizard.master.clone(),
            _ => return,
        };
        self.wizard.editing = true;
    }

    pub(crate) fn wizard_advance_step(&mut self) {
        use crate::setup_wizard::WizardStep;
        // Run setup actions for current step before advancing
        let dev_ops = PathBuf::from(&self.wizard.dev_ops);
        let master = PathBuf::from(&self.wizard.master);
        let tx = self.tx.clone();
        let github = self.wizard.github.clone();
        let skip_c = self.wizard.skip_claude;
        let skip_g = self.wizard.skip_gemini;
        let skip_a = self.wizard.skip_antigravity;

        match &self.wizard.step {
            WizardStep::Workspace if !dev_ops.as_os_str().is_empty() => {
                let tx2 = tx.clone();
                let d = dev_ops.clone();
                let gh = github.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_workspace(&d, &gh);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Master if !master.as_os_str().is_empty() => {
                let tx2 = tx.clone();
                let m = master.clone();
                let gh = github.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_master(&m, &gh);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Claude if !skip_c => {
                let tx2 = tx.clone();
                let d = dev_ops.clone();
                let m = master.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_claude(&d, &m);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Gemini if !skip_g => {
                let tx2 = tx.clone();
                let m = master.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_gemini(&m);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Antigravity if !skip_a => {
                let tx2 = tx.clone();
                let d = dev_ops.clone();
                let m = master.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_antigravity(&d, &m);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Skills if !dev_ops.as_os_str().is_empty() => {
                let tx2 = tx.clone();
                let d = dev_ops.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_skills(&d);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            _ => {}
        }

        self.wizard.step = self.wizard.step.next();
        self.wizard.field_cursor = 0;
    }

    pub(crate) fn wizard_run_initialize(&mut self) {
        let dev_ops = PathBuf::from(&self.wizard.dev_ops);
        let master = PathBuf::from(&self.wizard.master);
        let vault = if self.wizard.vault.is_empty() {
            None
        } else {
            Some(PathBuf::from(&self.wizard.vault))
        };
        let skills = dev_ops.join(".agents").join("skills");
        let tx = self.tx.clone();

        if dev_ops.as_os_str().is_empty() {
            return;
        }

        self.wizard.running = true;
        thread::spawn(move || {
            let actions =
                crate::setup_wizard::exec_initialize(&dev_ops, &master, &skills, vault.as_deref());
            tx.send(BgMsg::WizardActions(actions)).ok();
            tx.send(BgMsg::WizardDone).ok();
        });
    }
}
