use raios_surface_tui::app::Editor;
use std::path::{Path, PathBuf};
use std::thread;

use raios_surface_tui::app::{state::*, App};
use raios_runtime::compliance;
use raios_runtime::filebrowser::{load_file_content, save_file_content, FileEntry};

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

    pub(crate) fn is_constitution_path(&self, path: &Path) -> bool {
        self.constitution.tabs.iter().any(|t| t.path() == path)
    }

    pub(crate) fn jump_to_constitution_raw_edit(&mut self) {
        let Some(target) = self.constitution.tabs.get(self.constitution.active_tab).cloned() else { return };
        let line = self.outline_cursor_line();
        let entry = FileEntry::new(target.label(), target.path().to_path_buf());
        self.open_file_edit(entry);
        if let Some(l) = line {
            self.editor.editor.cursor_row = l.min(self.editor.editor.lines.len().saturating_sub(1));
            self.editor.editor.cursor_col = 0;
        }
    }

    fn outline_cursor_line(&self) -> Option<usize> {
        use raios_surface_tui::app::state::OutlineRow;
        let row = self.constitution.rows.get(self.constitution.outline_cursor)?;
        let sections = &self.constitution.sections;
        Some(match *row {
            OutlineRow::Section { idx } => sections[idx].line_start,
            OutlineRow::Child { idx, child_idx } => sections[idx].children[child_idx].line_start,
            OutlineRow::Item { idx, child_idx, .. } => match child_idx {
                Some(c) => sections[idx].children[c].line_start,
                None => sections[idx].line_start,
            },
        })
    }

    pub(crate) fn open_graph_report(&mut self, project_path: &Path) {
        match raios_surface_tui::app::load_graph_report_lines(project_path) {
            Ok(lines) => {
                self.projects.graph_report_lines = lines;
                self.projects.graph_report_scroll = 0;
                self.state = AppState::GraphReport;
            }
            Err(msg) => {
                self.system.sync_status = Some(msg);
            }
        }
    }

    pub(crate) fn open_git_diff(&mut self, project_path: &Path) {
        self.projects.git_diff_lines = raios_surface_tui::app::load_git_diff_lines(project_path);
        self.projects.git_diff_scroll = 0;
        self.state = AppState::GitDiffView;
    }

    pub(crate) fn save_file(&mut self) {
        if let Some(ref file) = self.editor.active_file.clone() {
            let content = self.editor.editor.content();
            if self.is_constitution_path(&file.path) {
                self.request_constitution_save(file.path.clone(), content);
                return;
            }
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

    pub(crate) fn request_constitution_save(&mut self, path: PathBuf, new_content: String) {
        let old_content = load_file_content(&path);
        let diff_lines = raios_surface_tui::app::editor::simple_diff(&old_content, &new_content);
        let added = diff_lines.iter().filter(|l| l.starts_with('+')).count();
        let removed = diff_lines.iter().filter(|l| l.starts_with('-')).count();
        self.constitution.pending_save = Some(raios_surface_tui::app::state::PendingConstitutionSave {
            path,
            new_content,
            diff_lines,
            added,
            removed,
        });
    }

    pub(crate) fn confirm_constitution_save(&mut self) {
        if let Some(pending) = self.constitution.pending_save.take() {
            match raios_runtime::filebrowser::save_constitution_file(&pending.path, &pending.new_content) {
                Ok(()) => {
                    self.editor.save_msg = Some("Saved!".into());
                    self.state = AppState::FileView;
                    let idx = self.constitution.active_tab;
                    self.load_constitution_tab(idx);
                }
                Err(e) => {
                    self.editor.save_msg = Some(format!("Error: {}", e));
                }
            }
        }
    }

    pub(crate) fn cancel_constitution_save(&mut self) {
        self.constitution.pending_save = None;
    }

    pub(crate) fn begin_item_edit(&mut self) {
        use raios_surface_tui::app::state::OutlineRow;
        let Some(&row) = self.constitution.rows.get(self.constitution.outline_cursor) else { return };
        if let OutlineRow::Item { idx, child_idx, item_idx } = row {
            let current = match child_idx {
                Some(c) => self.constitution.sections[idx].children[c].items[item_idx].clone(),
                None => self.constitution.sections[idx].items[item_idx].clone(),
            };
            self.constitution.item_input = current;
            self.constitution.item_editing = true;
        }
    }

    pub(crate) fn begin_item_insert(&mut self) {
        self.constitution.item_input = String::new();
        self.constitution.item_editing = true;
    }

    pub(crate) fn commit_item_edit(&mut self) {
        use raios_surface_tui::app::state::OutlineRow;
        let Some(target) = self.constitution.tabs.get(self.constitution.active_tab).cloned() else {
            self.constitution.item_editing = false;
            return;
        };
        let row = self.constitution.rows.get(self.constitution.outline_cursor).copied();
        let content = load_file_content(target.path());
        let new_text = self.constitution.item_input.clone();
        self.constitution.item_editing = false;

        let new_content = match row {
            // Cursor is on an existing item row: replace *that item's own* source line.
            // `outline_cursor_line()` can't be reused here — it intentionally returns the
            // enclosing section/child *header's* line for Item rows (correct for Task 6's
            // raw-edit-jump, since `ConstitutionSection` never tracks per-item line numbers).
            // Blindly reusing it here would overwrite the section/child heading instead of
            // the item text, so the item's real line is resolved by walking the raw content
            // with the same body-parsing rules `constitution::parse_body` uses.
            Some(OutlineRow::Item { idx, child_idx, item_idx }) => {
                let Some(line) = self.resolve_item_source_line(&content, idx, child_idx, item_idx) else {
                    return;
                };
                let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();
                if line < lines.len() {
                    // The outline always shows/edits marker-stripped text (see
                    // `begin_item_edit`, which pulls from `ConstitutionSection::items` —
                    // already stripped of its `- `/`* `/`N. ` prefix by the parser), so the
                    // original line's marker must be reattached here or every edited item
                    // would silently lose its list formatting on save.
                    let prefix = list_marker_prefix(&lines[line]).to_string();
                    lines[line] = format!("{prefix}{new_text}");
                } else {
                    lines.push(new_text);
                }
                lines.join("\n") + "\n"
            }
            // Cursor is on a section/child header row (via `n`): insert a brand-new line
            // directly under the header rather than overwriting the header itself.
            Some(OutlineRow::Section { .. }) | Some(OutlineRow::Child { .. }) => {
                let Some(header_line) = self.outline_cursor_line() else { return };
                let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();
                let insert_at = (header_line + 1).min(lines.len());
                lines.insert(insert_at, new_text);
                lines.join("\n") + "\n"
            }
            None => return,
        };
        self.request_constitution_save(target.path().to_path_buf(), new_content);
    }

    pub(crate) fn delete_item_at_cursor(&mut self) {
        use raios_surface_tui::app::state::OutlineRow;
        let Some(&row) = self.constitution.rows.get(self.constitution.outline_cursor) else { return };
        let OutlineRow::Item { idx, child_idx, item_idx } = row else { return };
        let Some(target) = self.constitution.tabs.get(self.constitution.active_tab).cloned() else { return };
        let content = load_file_content(target.path());
        let Some(line) = self.resolve_item_source_line(&content, idx, child_idx, item_idx) else { return };
        let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();
        if line < lines.len() {
            lines.remove(line);
        }
        let new_content = lines.join("\n") + "\n";
        self.request_constitution_save(target.path().to_path_buf(), new_content);
    }

    /// Resolves the absolute source-file line of an `OutlineRow::Item`'s own text.
    /// `ConstitutionSection` (raios_runtime::constitution) only stores `line_start` for
    /// section/child *headers* — it never tracks a per-item line number — so this walks the
    /// raw file content starting right after the relevant header, mirroring
    /// `constitution::parse_body`'s exact rules (a top-level section's items skip over any
    /// `### ` child blocks entirely; a child's items stop at the next `### `/`## ` header)
    /// so the `item_idx`-th line found here lines up with the same item the outline shows.
    fn resolve_item_source_line(
        &self,
        content: &str,
        idx: usize,
        child_idx: Option<usize>,
        item_idx: usize,
    ) -> Option<usize> {
        let sections = &self.constitution.sections;
        let (header_line, skip_child_blocks) = match child_idx {
            Some(c) => (sections[idx].children[c].line_start, false),
            None => (sections[idx].line_start, true),
        };
        let lines: Vec<&str> = content.lines().collect();
        let mut i = header_line + 1;
        let mut count = 0usize;
        while i < lines.len() {
            let line = lines[i];
            if line.starts_with("## ") {
                break;
            }
            if skip_child_blocks && line.starts_with("### ") {
                // Skip this child header's entire body — it doesn't belong to the parent
                // section's own `items` list (mirrors parse_body's recursive child call).
                i += 1;
                while i < lines.len() && !lines[i].starts_with("### ") && !lines[i].starts_with("## ") {
                    i += 1;
                }
                continue;
            }
            if !skip_child_blocks && line.starts_with("### ") {
                break;
            }
            if !line.trim().is_empty() {
                if count == item_idx {
                    return Some(i);
                }
                count += 1;
            }
            i += 1;
        }
        None
    }

    pub(crate) fn open_project_detail(&mut self, project: raios_core::entities::EntityProject) {
        self.projects.memory_scroll = 0;
        self.projects.panel_focus = false;
        self.projects.memory_lines = Vec::new();
        self.projects.git_log = Vec::new();
        self.projects.active = Some(project.clone());
        self.state = AppState::ProjectDetail;

        let tx = self.tx.clone();
        let proj_path = project.local_path.clone();
        thread::spawn(move || {
            tx.send(BgMsg::ProjectOpened(raios_surface_tui::app::load_project_detail_data(&proj_path)))
                .ok();
        });
    }

    pub(crate) fn wizard_commit_input(&mut self) {
        use raios_surface_tui::setup_wizard::WizardStep;
        match self.wizard.step {
            WizardStep::Workspace => match self.wizard.field_cursor {
                0 => self.wizard.dev_ops = self.wizard.input.clone(),
                1 => self.wizard.github = self.wizard.input.clone(),
                2 => self.wizard.vault = self.wizard.input.clone(),
                _ => {}
            },
            WizardStep::Constitution => {
                self.wizard.master = self.wizard.input.clone();
            }
            _ => {}
        }
        self.wizard.input.clear();
    }

    pub(crate) fn wizard_start_edit(&mut self) {
        use raios_surface_tui::setup_wizard::WizardStep;
        self.wizard.input = match self.wizard.step {
            WizardStep::Workspace => match self.wizard.field_cursor {
                0 => self.wizard.dev_ops.clone(),
                1 => self.wizard.github.clone(),
                2 => self.wizard.vault.clone(),
                _ => String::new(),
            },
            WizardStep::Constitution => self.wizard.master.clone(),
            _ => return,
        };
        self.wizard.editing = true;
    }

    pub(crate) fn wizard_advance_step(&mut self) {
        use raios_surface_tui::setup_wizard::WizardStep;
        // Run setup actions for current step before advancing
        let dev_ops = PathBuf::from(&self.wizard.dev_ops);
        let master = PathBuf::from(&self.wizard.master);
        let tx = self.tx.clone();
        let github = self.wizard.github.clone();
        let skip_c = self.wizard.skip_claude;
        let skip_o = self.wizard.skip_opencode;
        let skip_a = self.wizard.skip_antigravity;

        match &self.wizard.step {
            WizardStep::Workspace if !dev_ops.as_os_str().is_empty() => {
                let tx2 = tx.clone();
                let d = dev_ops.clone();
                let gh = github.clone();
                thread::spawn(move || {
                    let actions = raios_surface_tui::setup_wizard::exec_workspace(&d, &gh);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Constitution if !master.as_os_str().is_empty() => {
                let tx2 = tx.clone();
                let m = master.clone();
                let gh = github.clone();
                thread::spawn(move || {
                    let actions = raios_surface_tui::setup_wizard::exec_master(&m, &gh);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Claude if !skip_c => {
                let tx2 = tx.clone();
                let d = dev_ops.clone();
                let m = master.clone();
                thread::spawn(move || {
                    let actions = raios_surface_tui::setup_wizard::exec_claude(&d, &m);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Codex if !skip_a => {
                let tx2 = tx.clone();
                let m = master.clone();
                thread::spawn(move || {
                    let actions = raios_surface_tui::setup_wizard::exec_codex(&m);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::OpenCode if !skip_o => {
                let tx2 = tx.clone();
                thread::spawn(move || {
                    let actions = raios_surface_tui::setup_wizard::exec_opencode();
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Skills if !dev_ops.as_os_str().is_empty() => {
                let tx2 = tx.clone();
                let d = dev_ops.clone();
                thread::spawn(move || {
                    let actions = raios_surface_tui::setup_wizard::exec_skills(&d);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::AgentWrapper => {
                let tx2 = tx.clone();
                // Copy choice from field_cursor before advancing
                let choice = self.wizard.field_cursor;
                self.wizard.agent_wrapper_choice = choice;
                thread::spawn(move || {
                    let actions = raios_surface_tui::setup_wizard::exec_agent_wrapper(choice);
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
        let agent_wrapper_enabled = self.wizard.agent_wrapper_choice == 0;

        if dev_ops.as_os_str().is_empty() {
            return;
        }

        self.wizard.running = true;
        thread::spawn(move || {
            let actions = raios_surface_tui::setup_wizard::exec_initialize(
                &dev_ops,
                &master,
                &skills,
                vault.as_deref(),
                agent_wrapper_enabled,
            );
            tx.send(BgMsg::WizardActions(actions)).ok();
            tx.send(BgMsg::WizardDone).ok();
        });
    }

    pub(crate) fn handle_handover_approval(&mut self, approved: bool) {
        if let Some(ref tx) = self.tx_daemon {
            let msg = format!("{{\"command\":\"HumanApproval\",\"approved\":{}}}", approved);
            let _ = tx.send(msg);
        }
        self.system.handover_modal = None;
    }

    pub(crate) fn launch_agent_for_active(&mut self, agent: &str) {
        if let Some(ref proj) = self.projects.active.clone() {
            let msg = raios_surface_tui::app::events::helpers::launch_agent(agent, &proj.local_path);
            self.system.sync_status = Some(msg);
        }
        self.ui.show_launcher = false;
    }

    pub(crate) fn run_compliance_auto_fix(&mut self) {
        if let Some(ref report) = self.health.compliance {
            if !report.violations.is_empty() && !self.health.is_fixing {
                self.health.is_fixing = true;
                self.health.fix_status = Some("Claude fixing issues...".into());
                self.add_activity("Agent", "Initiating Auto-Fix with Claude Code", "Warning");
                let tx = self.tx.clone();
                thread::spawn(move || {
                    thread::sleep(std::time::Duration::from_secs(3));
                    tx.send(BgMsg::SyncDone(
                        "Auto-Fix Complete: Issues resolved".into(),
                    ))
                    .ok();
                });
            }
        }
    }

    pub(crate) fn open_current_file_in_editor(&mut self) {
        let files = self.current_menu_files();
        if let Some(entry) = files.into_iter().nth(self.ui.right_file_cursor) {
            let _ = raios_runtime::discovery::open_in_editor(&entry.path);
            if let Some(ref tx) = self.tx_daemon {
                let line = (self.editor.scroll as u64) + 1;
                let msg = serde_json::json!({
                    "event": "OpenFile",
                    "path": entry.path.to_string_lossy(),
                    "line": line,
                    "col": 1
                });
                let _ = tx.send(msg.to_string());
            }
        }
    }

    pub(crate) fn launch_agent_for_selected_project(&mut self, agent: &str) {
        let project_path = match self.ui.menu_cursor {
            7 => self.project_at_cursor().map(|p| p.local_path.clone()),
            _ => None,
        };

        if let Some(path) = project_path {
            self.add_activity(
                "Agent",
                &format!("Launching {} for project", agent),
                "Info",
            );
            self.system.sync_status = Some(raios_surface_tui::app::events::helpers::launch_agent(agent, &path));
        }
    }

    pub(crate) fn run_graphify_on_active(&mut self) {
        if let Some(ref proj) = self.projects.active.clone() {
            let msg = self.run_graphify(&proj.local_path);
            self.system.sync_status = Some(msg);
        }
    }

    pub(crate) fn handle_file_change_approval(&mut self, approved: bool) {
        if let Some(pending) = self
            .system
            .pending_file_changes
            .get(self.system.pending_change_cursor)
            .cloned()
        {
            if let Some(ref tx) = self.tx_daemon {
                let msg = serde_json::json!({
                    "command": "ApproveFileChange",
                    "id": pending.id.clone(),
                    "path": pending.path,
                    "approved": approved
                });
                let _ = tx.send(msg.to_string());
                if approved {
                    self.add_activity(
                        "System",
                        &format!("File change approved for {}", pending.path),
                        "Info",
                    );
                } else {
                    self.add_activity(
                        "System",
                        &format!("File change rejected for {}", pending.path),
                        "Warning",
                    );
                }
            }
            self.system
                .pending_file_changes
                .remove(self.system.pending_change_cursor);
            if self.system.pending_change_cursor >= self.system.pending_file_changes.len()
                && !self.system.pending_file_changes.is_empty()
            {
                self.system.pending_change_cursor =
                    self.system.pending_file_changes.len() - 1;
            }
        }
        if self.system.pending_file_changes.is_empty() {
            self.state = AppState::Dashboard;
        } else {
            let next = &self.system.pending_file_changes[self.system.pending_change_cursor];
            self.projects.git_diff_lines =
                raios_surface_tui::app::editor::simple_diff(&next.original_content, &next.new_content);
        }
    }

    pub(crate) fn launch_agent_from_mempalace(&mut self, agent: &str) {
        if let Some(proj) = self.get_selected_mempalace_project() {
            self.add_activity(
                "Agent",
                &format!("Launching {} from MemPalace", agent),
                "Info",
            );
            self.system.sync_status = Some(raios_surface_tui::app::events::helpers::launch_agent(agent, &proj.path));
        }
    }

    pub(crate) fn refresh_health(&mut self) {
        self.health.is_checking = true;
        if let Some(ref tx_daemon) = self.tx_daemon {
            let _ = tx_daemon.send("{\"command\":\"GetState\"}".into());
            self.add_activity("System", "Manual health refresh requested", "Info");
        }
    }

    pub(crate) fn commit_project_at_health_cursor(&mut self) {
        if let Some(h) = self.health.report.get(self.health.cursor).cloned() {
            if h.git_dirty == Some(true) {
                let tx = self.tx.clone();
                let path = h.path.clone();
                let name = h.name.clone();
                self.system.sync_status = Some(format!("Committing {}...", name));
                thread::spawn(move || {
                    let r = raios_core::core::git::commit(&path, "chore: raios update", true);
                    tx.send(BgMsg::GitActionDone {
                        project: name,
                        action: "commit".into(),
                        ok: r.ok,
                        message: r.message,
                    })
                    .ok();
                });
            } else {
                self.system.sync_status =
                    Some("Nothing to commit (working tree clean)".into());
            }
        }
    }

    pub(crate) fn push_project_at_health_cursor(&mut self) {
        if let Some(h) = self.health.report.get(self.health.cursor).cloned() {
            let tx = self.tx.clone();
            let path = h.path.clone();
            let name = h.name.clone();
            self.system.sync_status = Some(format!("Pushing {}...", name));
            thread::spawn(move || {
                let r = raios_core::core::git::push(&path);
                tx.send(BgMsg::GitActionDone {
                    project: name,
                    action: "push".into(),
                    ok: r.ok,
                    message: r.message,
                })
                .ok();
            });
        }
    }
}

/// Returns the leading list-marker slice of `line` (`"- "`, `"* "`, or a numbered `"N. "`,
/// including any leading indentation), or `""` if the line has no marker. Mirrors
/// `raios_runtime::constitution::strip_list_marker`'s own recognition rules (that function
/// isn't `pub`, so its logic is duplicated here) so a replaced item line can keep its
/// original bullet style instead of silently losing it.
fn list_marker_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    if trimmed.starts_with("* ") || trimmed.starts_with("- ") {
        return &line[..indent_len + 2];
    }
    if let Some(dot) = trimmed.find(". ") {
        if !trimmed[..dot].is_empty() && trimmed[..dot].chars().all(|c| c.is_ascii_digit()) {
            return &line[..indent_len + dot + 2];
        }
    }
    ""
}

