use anyhow::Result;
use raios_contracts::FactoryCommand;
use raios_runtime::filebrowser::find_file_by_name;
use raios_runtime::sync::sync_universe;
use std::thread;

use raios_surface_tui::app::events::helpers::append_memo;
use raios_surface_tui::app::intent::Intent;
use raios_surface_tui::app::reducer::reduce_intent;
use raios_surface_tui::app::route::Route;
use raios_surface_tui::app::{state::*, App};

impl App {
    pub(crate) fn execute_command(&mut self, raw: &str) -> Result<()> {
        let raw = raw.trim();
        if !raw.starts_with('/') {
            return Ok(());
        }
        let parts: Vec<&str> = raw.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).copied().unwrap_or("").trim();

        match cmd {
            "/now" | "/approvals" | "/inbox" => {
                reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Now));
            }
            "/work" | "/projects" | "/tasks" => {
                reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Work));
            }
            "/explore" | "/traces" => {
                reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Explore));
            }
            "/govern" | "/policy" | "/cron" => {
                reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Govern));
            }
            "/refresh" => {
                reduce_intent(&mut self.store, Intent::RefreshSnapshot);
                if let Err(problem) = self
                    .client
                    .send_query(raios_contracts::Query::GetSystemSnapshot)
                {
                    self.store.last_error = Some(problem.message);
                }
            }
            "/factory" => match factory_command_from_input(arg) {
                Ok(command) => {
                    if let Err(problem) = self.client.send_factory_command(command) {
                        self.store.last_error = Some(problem.message);
                    } else {
                        self.system.sync_status =
                            Some("Factory command sent; waiting for audited result.".into());
                    }
                }
                Err(usage) => self.system.sync_status = Some(usage),
            },
            "/sync" | "/setup" => {
                self.system.is_syncing = true;
                self.system.sync_status = None;
                self.add_activity("System", "Starting Universal Sync...", "Info");
                let tx = self.tx.clone();
                let dev = self.config.dev_ops_path.clone();
                let mst = self.config.master_md_path.clone();
                thread::spawn(move || match sync_universe(&dev, &mst) {
                    Ok(msg) => tx.send(BgMsg::SyncDone(msg)).ok(),
                    Err(e) => tx.send(BgMsg::SyncError(e.to_string())).ok(),
                });
            }
            "/discover" => {
                if self.is_remote {
                    if let Some(ref tx) = self.tx_daemon {
                        let _ = tx.send("{\"command\":\"GetState\"}".into());
                        self.system.sync_status = Some("Refreshing remote state...".into());
                    }
                } else {
                    let projects =
                        raios_core::entities::discover_entities(&self.config.dev_ops_path);
                    self.projects.list = projects.clone();
                    match raios_core::entities::save_entities(&self.config.dev_ops_path, projects) {
                        Ok(_) => {
                            self.system.sync_status =
                                Some("Discovery complete: entities.json updated".into())
                        }
                        Err(e) => self.system.sync_status = Some(format!("Discovery error: {}", e)),
                    }
                    self.add_activity("System", "Full Dev Ops discovery complete", "Info");
                }
            }
            "/q" | "/quit" | "/exit" => self.should_quit = true,
            "/rules" => {
                self.ui.menu_cursor = 1;
                self.ui.right_panel_focus = true;
                self.ui.right_file_cursor = 0;
            }
            "/memory" => {
                self.ui.menu_cursor = 5;
                self.ui.right_panel_focus = true;
                self.ui.right_file_cursor = 2;
            }
            "/mempalace" | "/palace" | "/mp" => {
                if self.mempalace.rooms.is_empty() && !self.mempalace.is_building {
                    self.mempalace.is_building = true;
                    let tx = self.tx.clone();
                    let dev_ops = self.config.dev_ops_path.clone();
                    thread::spawn(move || {
                        tx.send(BgMsg::MemPalaceBuilt(raios_core::mempalace::build(
                            &dev_ops,
                        )))
                        .ok();
                    });
                }
                self.state = AppState::MemPalaceView;
                self.mempalace.filter.clear();
            }
            "/view" if !arg.is_empty() => {
                if let Some(entry) = find_file_by_name(arg, &self.config.master_md_path) {
                    self.open_file_view(entry);
                }
            }
            "/edit" if !arg.is_empty() => {
                if let Some(entry) = find_file_by_name(arg, &self.config.master_md_path) {
                    if !entry.read_only {
                        self.open_file_edit(entry);
                    }
                }
            }
            "/search" | "/s" => {
                self.ui.menu_cursor = 6;
                self.ui.right_panel_focus = false;
                if !arg.is_empty() {
                    self.add_activity("User", &format!("Searching for: {}", arg), "Info");
                    if self.is_remote {
                        if let Some(ref tx) = self.tx_daemon {
                            let cmd = raios_surface_tui::app::daemon_search_command(arg);
                            let _ = tx.send(cmd);
                            self.search.status = Some(format!("Searching remote hub: {}", arg));
                        }
                    } else if let Some(ref idx) = self.search.index {
                        self.search.results = idx.search(arg);
                        self.search.cursor = 0;
                        if !self.search.results.is_empty() {
                            self.ui.right_panel_focus = true;
                        }
                    } else {
                        self.search.status = Some("Index not ready — try again shortly".into());
                    }
                }
            }
            "/memo" | "/note" if !arg.is_empty() => {
                let result = append_memo(arg, &self.config.dev_ops_path);
                self.system.sync_status = Some(result);
            }
            "/scan-system" | "/audit" => {
                self.system.is_scanning = true;
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let report = raios_runtime::system_scan::scan_system();
                    tx.send(BgMsg::AiAuditReport(report)).ok();
                });
            }
            "/open" | "/project" => {
                if !arg.is_empty() {
                    let q = arg.to_lowercase();
                    if let Some(proj) = self
                        .projects
                        .list
                        .iter()
                        .find(|p| p.name.to_lowercase().contains(&q))
                        .cloned()
                    {
                        self.open_project_detail(proj);
                    } else {
                        self.system.sync_status = Some(format!("Project not found: {}", arg));
                    }
                } else {
                    self.ui.menu_cursor = 7;
                    self.ui.right_panel_focus = false;
                }
            }
            "/timeline" | "/history" => {
                self.ui.menu_cursor = 8;
                self.ui.right_panel_focus = false;
            }
            "/logs" | "/log" => {
                self.ui.menu_cursor = 9;
                self.ui.right_panel_focus = false;
                // Request fresh log replay from daemon (restores history after reconnect)
                if let Some(ref tx) = self.tx_daemon {
                    let limit = if arg.is_empty() {
                        200
                    } else {
                        arg.parse::<u64>().unwrap_or(200)
                    };
                    let _ = tx.send(raios_surface_tui::app::daemon_get_logs_command(limit));
                }
            }
            "/help" | "/?" => {
                self.state = AppState::HelpView;
            }
            "/task" => {
                // /task add <text> [@agent] [#project]
                // /task send claude|antigravity
                if arg.starts_with("add ") {
                    let rest = arg.trim_start_matches("add ").trim();
                    // Parse the line using the same parser (add checkbox prefix)
                    let fake_line = format!("- [ ] {}", rest);
                    // Re-use load logic: parse inline
                    let new_task = raios_runtime::tasks::parse_task_line(&fake_line)
                        .unwrap_or_else(|| raios_runtime::tasks::Task {
                            id: None,
                            text: rest.to_string(),
                            completed: false,
                            agent: None,
                            project: None,
                        });
                    let agent_hint = new_task.agent.as_deref().unwrap_or("-");
                    let proj_hint = new_task.project.as_deref().unwrap_or("-");
                    self.system.sync_status =
                        Some(format!("Task added [{}→{}]", agent_hint, proj_hint));
                    self.tasks.list.push(new_task);
                    let _ = raios_runtime::tasks::save_tasks(
                        &self.config.dev_ops_path,
                        &self.tasks.list,
                    );
                } else if let Some(agent) = arg.strip_prefix("send ") {
                    self.dispatch_task(agent.trim());
                } else if arg == "load" {
                    if let Ok(tasks) = raios_runtime::tasks::load_tasks(&self.config.dev_ops_path) {
                        self.tasks.list = tasks;
                        self.system.sync_status =
                            Some(format!("{} tasks loaded", self.tasks.list.len()));
                    }
                }
            }
            "/vault-create" => {
                let name = arg.trim();
                if name.is_empty() {
                    self.system.sync_status = Some("Usage: /vault-create <project_name>".into());
                } else {
                    let proj = self.projects.list.iter().find(|p| p.name == name).cloned();
                    if let Some(p) = proj {
                        match raios_surface_tui::app::create_vault_note(
                            &self.config.vault_projects_path,
                            &p,
                        ) {
                            Ok(true) => {
                                self.system.vault_projects.push(p.name.clone());
                                self.system.sync_status =
                                    Some(format!("Vault note created: {}", p.name));
                                self.add_activity(
                                    "System",
                                    &format!("Created vault note for {}", p.name),
                                    "Info",
                                );
                            }
                            Ok(false) => {
                                self.system.sync_status = Some("Vault note already exists".into());
                            }
                            Err(_) => {
                                self.system.sync_status = Some("Failed to write vault note".into());
                            }
                        }
                    } else {
                        self.system.sync_status = Some(format!("Project not found: {}", name));
                    }
                }
            }
            "/health" => {
                self.health.is_checking = true;
                self.state = AppState::HealthView;
                if self.is_remote {
                    if let Some(ref tx) = self.tx_daemon {
                        let _ = tx.send("{\"command\":\"GetState\"}".into());
                        let _ = tx.send("{\"command\":\"HealthScan\"}".into());
                        self.system.sync_status = Some("Scanning remote hub health...".into());
                    }
                } else if !self.projects.list.is_empty() {
                    if let Some(ref tx_daemon) = self.tx_daemon {
                        let _ = tx_daemon.send("{\"command\":\"GetState\"}".into());
                    }
                } else {
                    self.system.sync_status = Some("Load entities.json first".into());
                }
            }
            "/reindex" => {
                self.add_activity("System", "Requesting search re-index", "Info");
                if self.config.dev_ops_path.exists() && !self.search.is_indexing {
                    self.search.is_indexing = true;
                    self.search.status = Some("Rebuilding index...".into());
                    let tx = self.tx.clone();
                    let dev_ops = self.config.dev_ops_path.clone();
                    thread::spawn(move || {
                        match raios_runtime::indexer::ProjectIndex::build(&dev_ops) {
                            Ok(idx) => tx.send(BgMsg::IndexReady(idx)).ok(),
                            Err(e) => tx.send(BgMsg::IndexError(e.to_string())).ok(),
                        };
                    });
                }
            }
            "/graphify" | "/graph" => {
                if let Some(ref proj) = self.projects.active.clone() {
                    let msg = self.run_graphify(&proj.local_path);
                    self.system.sync_status = Some(msg);
                } else {
                    self.system.sync_status =
                        Some("Open a project detail first to run graphify".into());
                }
            }
            "/heal" => {
                if let Some(ref proj) = self.projects.active.clone() {
                    let mut sentinel_errors = Vec::new();
                    let path_str = proj.local_path.to_string_lossy().to_string();
                    for file in &self.system.sentinel_files {
                        if file.path.contains(&path_str)
                            && file.state == raios_runtime::sentinel::SentinelState::Failed
                        {
                            for err in &file.errors {
                                sentinel_errors.push(format!(
                                    "{}:{}: {}",
                                    err.file,
                                    err.line.unwrap_or(0),
                                    err.message
                                ));
                            }
                        }
                    }

                    if sentinel_errors.is_empty() {
                        self.system.sync_status =
                            Some("No sentinel errors detected in current project.".into());
                    } else {
                        // Trigger correction task
                        let task_text = format!("FIX SENTINEL ERRORS: {}", sentinel_errors[0]);
                        let task = raios_runtime::tasks::Task {
                            id: None,
                            text: task_text,
                            completed: false,
                            agent: Some("claude".into()),
                            project: Some(proj.name.clone()),
                        };
                        let result = raios_runtime::tasks::dispatch_to_agent(
                            &task,
                            "claude",
                            Some(&proj.local_path),
                            Some(sentinel_errors),
                        );
                        self.system.sync_status = Some(result);
                        self.add_activity("Sentinel", "Self-Correction cycle triggered", "Warning");
                    }
                } else {
                    self.system.sync_status =
                        Some("Open a project detail first to run /heal".into());
                }
            }
            "/ext" | "/extensions" => {
                self.ui.menu_cursor = 15;
                self.ui.right_panel_focus = false;
                if !self.ext.loaded {
                    self.load_extensions();
                }
            }
            // Remote-only: execute a raios subcommand on the hub server
            // Usage: /run health myproject | /run reflect | /run pre-flight kaira-mix
            "/run" if self.is_remote && !arg.is_empty() => {
                if let Some(ref tx) = self.tx_daemon {
                    let cmd = raios_surface_tui::app::daemon_submit_raios_command(arg);
                    let _ = tx.send(cmd);
                    self.system.sync_status = Some(format!("→ Remote: raios {}", arg));
                    self.add_activity("Remote", &format!("raios {}", arg), "Info");
                } else {
                    self.system.sync_status = Some("Not connected to remote hub".into());
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn factory_command_from_input(input: &str) -> Result<FactoryCommand, String> {
    let mut parts = input.trim().splitn(2, ' ');
    let action = parts.next().unwrap_or_default();
    let payload = parts.next().unwrap_or_default().trim();
    let idempotency_key = format!("factory-tui-{}", uuid::Uuid::new_v4());

    match action {
        "workspace" if !payload.is_empty() => Ok(FactoryCommand::CreateWorkspace {
            name: payload.into(),
            idempotency_key,
        }),
        "product" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::CreateProductDraft {
                workspace_id: fields[0].into(),
                title: fields[1].into(),
                idempotency_key,
            })
        }
        "mode" => {
            let fields = split_factory_fields(payload, 2)?;
            let mode = match fields[1] {
                "quick" => raios_contracts::FactoryMode::Quick,
                "governed" => raios_contracts::FactoryMode::Governed,
                _ => return Err(factory_usage()),
            };
            Ok(FactoryCommand::SetProductMode {
                product_id: fields[0].into(),
                mode,
                idempotency_key,
            })
        }
        "scaffold" if !payload.is_empty() => Ok(FactoryCommand::ScaffoldProject {
            product_id: payload.into(),
            idempotency_key,
        }),
        "intake" if !payload.is_empty() => Ok(FactoryCommand::StartIntake {
            product_id: payload.into(),
            idempotency_key,
        }),
        "answer" => {
            let fields = split_factory_fields(payload, 3)?;
            Ok(FactoryCommand::RecordIntakeAnswer {
                session_id: fields[0].into(),
                question_key: fields[1].into(),
                response: fields[2].into(),
                idempotency_key,
            })
        }
        "charter" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::CreateCharterDraft {
                product_id: fields[0].into(),
                content: fields[1].into(),
                idempotency_key,
            })
        }
        "generate" if !payload.is_empty() => Ok(FactoryCommand::GenerateCharterDraft {
            product_id: payload.into(),
            idempotency_key,
        }),
        "requirement" => {
            let fields = split_factory_fields(payload, 3)?;
            Ok(FactoryCommand::CreateRequirementDraft {
                product_id: fields[0].into(),
                stable_key: fields[1].into(),
                content: fields[2].into(),
                idempotency_key,
            })
        }
        "change" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::SubmitChangeRequest {
                product_id: fields[0].into(),
                summary: fields[1].into(),
                idempotency_key,
            })
        }
        "assess" if !payload.is_empty() => Ok(FactoryCommand::AssessChangeRequest {
            change_request_id: payload.into(),
            idempotency_key,
        }),
        "plan" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::CreatePlanDraft {
                product_id: fields[0].into(),
                title: fields[1].into(),
                idempotency_key,
            })
        }
        "approve-plan" if !payload.is_empty() => Ok(FactoryCommand::ApprovePlan {
            plan_id: payload.into(),
            idempotency_key,
        }),
        "cycle" if !payload.is_empty() => Ok(FactoryCommand::MaterializePlannedCycle {
            plan_id: payload.into(),
            idempotency_key,
        }),
        "pause-cycle" if !payload.is_empty() => Ok(FactoryCommand::PauseCycle {
            cycle_id: payload.into(),
            idempotency_key,
        }),
        "resume-cycle" if !payload.is_empty() => Ok(FactoryCommand::ResumeCycle {
            cycle_id: payload.into(),
            idempotency_key,
        }),
        "cancel-cycle" if !payload.is_empty() => Ok(FactoryCommand::CancelCycle {
            cycle_id: payload.into(),
            idempotency_key,
        }),
        "stage-graph" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::MaterializeStageTaskGraph {
                cycle_id: fields[0].into(),
                stage: fields[1].into(),
                idempotency_key,
            })
        }
        "activate-stage" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::ActivateApprovedStage {
                cycle_id: fields[0].into(),
                stage: fields[1].into(),
                idempotency_key,
            })
        }
        "stage-evidence" => {
            let fields = split_factory_fields(payload, 3)?;
            Ok(FactoryCommand::RecordStageEvidence {
                cycle_id: fields[0].into(),
                stage: fields[1].into(),
                content_ref: fields[2].into(),
                idempotency_key,
            })
        }
        "link-evidence" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::LinkStageEvidenceToRequirement {
                evidence_id: fields[0].into(),
                requirement_id: fields[1].into(),
                idempotency_key,
            })
        }
        "complete-stage" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::CompleteStage {
                cycle_id: fields[0].into(),
                stage: fields[1].into(),
                idempotency_key,
            })
        }
        "readiness" if !payload.is_empty() => Ok(FactoryCommand::InspectReleaseReadiness {
            product_id: payload.into(),
            idempotency_key,
        }),
        "quality" => {
            let fields = split_factory_fields(payload, 3)?;
            let required = match fields[2] {
                "required" => true,
                "optional" => false,
                _ => return Err(factory_usage()),
            };
            Ok(FactoryCommand::CreateQualityProfile {
                product_id: fields[0].into(),
                name: fields[1].into(),
                required,
                idempotency_key,
            })
        }
        "rn-quality" if !payload.is_empty() => Ok(
            FactoryCommand::EnsureReactNativeClosedTestingQualityProfile {
                product_id: payload.into(),
                idempotency_key,
            },
        ),
        "check" => {
            let fields = split_factory_fields(payload, 3)?;
            let passed = match fields[1] {
                "passed" => true,
                "failed" => false,
                _ => return Err(factory_usage()),
            };
            Ok(FactoryCommand::RecordQualityCheck {
                profile_id: fields[0].into(),
                passed,
                evidence_ref: fields[2].into(),
                idempotency_key,
            })
        }
        "release" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::CreateReleaseDraft {
                product_id: fields[0].into(),
                build_ref: fields[1].into(),
                idempotency_key,
            })
        }
        "approve-release" if !payload.is_empty() => {
            Ok(FactoryCommand::ApproveClosedTestingRelease {
                release_id: payload.into(),
                idempotency_key,
            })
        }
        "support" => {
            let fields = split_factory_fields(payload, 3)?;
            Ok(FactoryCommand::CreateSupportItem {
                product_id: fields[0].into(),
                source_kind: fields[1].into(),
                summary: fields[2].into(),
                idempotency_key,
            })
        }
        "support-report" if !payload.is_empty() => Ok(FactoryCommand::InspectSupportOverview {
            product_id: payload.into(),
            idempotency_key,
        }),
        "triage" if !payload.is_empty() => Ok(FactoryCommand::TriageSupportItem {
            support_item_id: payload.into(),
            idempotency_key,
        }),
        "resolve-support" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::ResolveSupportItem {
                support_item_id: fields[0].into(),
                resolution_ref: fields[1].into(),
                idempotency_key,
            })
        }
        "link-support" => {
            let fields = split_factory_fields(payload, 2)?;
            Ok(FactoryCommand::LinkSupportToChangeRequest {
                support_item_id: fields[0].into(),
                change_request_id: fields[1].into(),
                idempotency_key,
            })
        }
        _ => Err(factory_usage()),
    }
}

fn split_factory_fields(input: &str, expected: usize) -> Result<Vec<&str>, String> {
    let fields: Vec<&str> = input.split('|').map(str::trim).collect();
    if fields.len() == expected && fields.iter().all(|field| !field.is_empty()) {
        Ok(fields)
    } else {
        Err(factory_usage())
    }
}

fn factory_usage() -> String {
    "Usage: /factory workspace <name> | product <workspace_id> | <title> | mode <product_id> | quick|governed | scaffold <product_id> | intake <product_id> | answer <session_id> | <question_key> | <response> | charter <product_id> | <content> | generate <product_id> | requirement <product_id> | <stable_key> | <content> | change <product_id> | <summary> | assess <change_request_id> | plan <product_id> | <title> | approve-plan <plan_id> | cycle <plan_id> | pause-cycle <cycle_id> | resume-cycle <cycle_id> | cancel-cycle <cycle_id> | stage-graph <cycle_id> | <stage> | activate-stage <cycle_id> | <stage> | stage-evidence <cycle_id> | <stage> | <content_ref> | link-evidence <evidence_id> | <requirement_id> | complete-stage <cycle_id> | <stage> | readiness <product_id> | quality <product_id> | <name> | required|optional | rn-quality <product_id> | check <profile_id> | passed|failed | <evidence_ref> | release <product_id> | <build_ref> | approve-release <release_id> | support <product_id> | <source_kind> | <summary> | triage <support_item_id> | resolve-support <support_item_id> | <resolution_ref> | link-support <support_item_id> | <change_request_id>".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_palette_parser_builds_bounded_contract_variants() {
        let product =
            factory_command_from_input("product workspace-1 | React Native pilot").unwrap();
        assert!(
            matches!(product, FactoryCommand::CreateProductDraft { workspace_id, title, .. } if workspace_id == "workspace-1" && title == "React Native pilot")
        );
        let answer =
            factory_command_from_input("answer session-1 | target_user | Independent builders")
                .unwrap();
        assert!(
            matches!(answer, FactoryCommand::RecordIntakeAnswer { question_key, response, .. } if question_key == "target_user" && response == "Independent builders")
        );
        assert!(factory_command_from_input("charter product-1").is_err());
        assert!(matches!(
            factory_command_from_input("mode product-1 | quick"),
            Ok(FactoryCommand::SetProductMode {
                mode: raios_contracts::FactoryMode::Quick,
                ..
            })
        ));
        assert!(matches!(
            factory_command_from_input("generate product-1").unwrap(),
            FactoryCommand::GenerateCharterDraft { product_id, .. } if product_id == "product-1"
        ));
        assert!(matches!(
            factory_command_from_input("change product-1 | Add export flow").unwrap(),
            FactoryCommand::SubmitChangeRequest { product_id, summary, .. } if product_id == "product-1" && summary == "Add export flow"
        ));
        assert!(matches!(
            factory_command_from_input("link-support support-1 | change-1").unwrap(),
            FactoryCommand::LinkSupportToChangeRequest { support_item_id, change_request_id, .. } if support_item_id == "support-1" && change_request_id == "change-1"
        ));
        assert!(matches!(
            factory_command_from_input("readiness product-1").unwrap(),
            FactoryCommand::InspectReleaseReadiness { product_id, .. } if product_id == "product-1"
        ));
        assert!(matches!(
            factory_command_from_input("pause-cycle cycle-1").unwrap(),
            FactoryCommand::PauseCycle { cycle_id, .. } if cycle_id == "cycle-1"
        ));
        assert!(matches!(
            factory_command_from_input("link-evidence evidence-1 | requirement-1").unwrap(),
            FactoryCommand::LinkStageEvidenceToRequirement { evidence_id, requirement_id, .. } if evidence_id == "evidence-1" && requirement_id == "requirement-1"
        ));
        assert!(matches!(
            factory_command_from_input("rn-quality product-1").unwrap(),
            FactoryCommand::EnsureReactNativeClosedTestingQualityProfile { product_id, .. } if product_id == "product-1"
        ));
    }
}
