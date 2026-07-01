use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use raios_surface_tui::app::{
    state::{BgMsg, ExtCmdInfo, ExtConfigField, ExtServiceStatus, ExtensionInfo, SortMode},
    App,
};
use raios_core::db::{RunOverviewRow, ScoredApproval, ScheduledJob, UnifiedTaskRow};
use raios_core::entities::EntityProject;
use raios_runtime::health::ProjectHealth;

#[derive(Debug, Default, Clone)]
pub struct InboxPanelData {
    pub approvals: Vec<ScoredApproval>,
    pub runs: Vec<RunOverviewRow>,
    pub blocked: Vec<UnifiedTaskRow>,
}

#[derive(Debug, Default, Clone)]
pub struct SchedulerPanelData {
    pub jobs: Vec<ScheduledJob>,
}

#[derive(Debug, Clone)]
pub struct PoliciesPanelData {
    pub policy: Option<raios_core::security::PolicyConfig>,
    pub audit_count: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ProjectListItem {
    pub name: String,
    pub has_vault: bool,
    pub status: String,
    pub category: String,
    pub compliance_grade: String,
    pub dirty: Option<bool>,
    pub ci_status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectsPanelData {
    pub total: usize,
    pub sort_label: &'static str,
    pub items: Vec<ProjectListItem>,
}

#[derive(Debug, Default, Clone)]
pub struct ProjectDetailData {
    pub memory_lines: Vec<String>,
    pub git_log: Vec<String>,
}

pub fn load_inbox_panel_data() -> Result<InboxPanelData, String> {
    let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
    load_inbox_panel_data_from_conn(&conn).map_err(|e| e.to_string())
}

fn load_inbox_panel_data_from_conn(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<InboxPanelData> {
    Ok(InboxPanelData {
        approvals: raios_core::db::cp_query_pending_approvals_scored(conn)?,
        runs: raios_core::db::cp_query_active_runs(conn)?,
        blocked: raios_core::db::cp_query_blocked_tasks(conn)?,
    })
}

pub fn load_scheduler_panel_data() -> Result<SchedulerPanelData, String> {
    let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
    Ok(SchedulerPanelData {
        jobs: raios_core::db::cp_scheduled_jobs_list(&conn).map_err(|e| e.to_string())?,
    })
}

pub fn load_policies_panel_data() -> PoliciesPanelData {
    let policy = raios_core::security::PolicyConfig::try_load_default();
    let audit_count = raios_core::db::open_db()
        .ok()
        .and_then(|conn| {
            conn.query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
                .ok()
        });
    PoliciesPanelData { policy, audit_count }
}

pub fn sort_project_indices(
    projects: &[EntityProject],
    health: &[ProjectHealth],
    sort: &SortMode,
) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..projects.len()).collect();
    match sort {
        SortMode::Name => indices.sort_by(|&a, &b| {
            projects[a]
                .name
                .to_lowercase()
                .cmp(&projects[b].name.to_lowercase())
        }),
        SortMode::Grade => indices.sort_by(|&a, &b| {
            let get_grade = |p: &EntityProject| {
                health
                    .iter()
                    .find(|h| h.name == p.name)
                    .map(|h| h.compliance_grade.as_str())
                    .unwrap_or("Z")
            };
            get_grade(&projects[a]).cmp(get_grade(&projects[b]))
        }),
        SortMode::GitDirty => indices.sort_by(|&a, &b| {
            let get_dirty = |p: &EntityProject| {
                health
                    .iter()
                    .find(|h| h.name == p.name)
                    .and_then(|h| h.git_dirty)
                    .unwrap_or(false)
            };
            get_dirty(&projects[b]).cmp(&get_dirty(&projects[a]))
        }),
        SortMode::Category => indices.sort_by(|&a, &b| projects[a].category.cmp(&projects[b].category)),
        SortMode::Status => indices.sort_by(|&a, &b| projects[a].status.cmp(&projects[b].status)),
    }
    indices
}

pub fn build_projects_panel_data(app: &App) -> ProjectsPanelData {
    let indices = sort_project_indices(&app.projects.list, &app.health.report, &app.projects.sort);
    let items = indices
        .into_iter()
        .map(|orig_i| {
            let proj = &app.projects.list[orig_i];
            let health = app.health.report.iter().find(|h| h.name == proj.name);
            ProjectListItem {
                name: proj.name.clone(),
                has_vault: app.system.vault_projects.contains(&proj.name),
                status: proj.status.clone(),
                category: proj.category.replace('_', " "),
                compliance_grade: health
                    .map(|h| h.compliance_grade.clone())
                    .unwrap_or_else(|| "-".into()),
                dirty: health.and_then(|h| h.git_dirty),
                ci_status: health.and_then(|h| h.ci_status.clone()),
            }
        })
        .collect();
    ProjectsPanelData {
        total: app.projects.list.len(),
        sort_label: app.projects.sort.label(),
        items,
    }
}

pub fn load_project_detail_data(project_path: &Path) -> ProjectDetailData {
    let memory_path = project_path.join("memory.md");
    let content = raios_runtime::filebrowser::load_file_content(&memory_path);
    ProjectDetailData {
        memory_lines: content.lines().map(str::to_owned).collect(),
        git_log: raios_runtime::filebrowser::get_git_log(project_path),
    }
}

pub fn load_graph_report_lines(project_path: &Path) -> Result<Vec<String>, String> {
    let report_path = project_path.join("GRAPH_REPORT.md");
    if !report_path.exists() {
        return Err("Graph report not found. Run Graphify first.".into());
    }
    Ok(raios_runtime::filebrowser::load_file_content(&report_path)
        .lines()
        .map(str::to_owned)
        .collect())
}

pub fn load_git_diff_lines(project_path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .current_dir(project_path)
        .args(["diff"])
        .output();

    match output {
        Ok(out) => {
            let diff = String::from_utf8_lossy(&out.stdout).to_string();
            if diff.trim().is_empty() {
                vec!["No unstaged changes.".to_string()]
            } else {
                diff.lines().map(|s| s.to_string()).collect()
            }
        }
        Err(_) => vec!["Failed to run git diff.".to_string()],
    }
}

pub fn daemon_search_command(query: &str) -> String {
    format!(
        "{{\"command\":\"Search\",\"query\":\"{}\"}}",
        query.replace('"', "\\\"")
    )
}

pub fn daemon_get_logs_command(limit: u64) -> String {
    format!("{{\"command\":\"GetLogs\",\"limit\":{}}}", limit)
}

pub fn daemon_submit_raios_command(args: &str) -> String {
    let escaped = args.replace('"', "\\\"");
    let shell_cmd = format!("raios {}", escaped);
    format!(
        "{{\"command\":\"SubmitJob\",\"shell_cmd\":\"{}\",\"description\":\"raios {}\",\"agent\":\"tui\"}}",
        shell_cmd, escaped
    )
}

pub fn create_vault_note(
    vault_projects_path: &Path,
    project: &EntityProject,
) -> std::io::Result<bool> {
    let vault_file = vault_projects_path.join(format!("{}.md", project.name));
    if vault_file.exists() {
        return Ok(false);
    }

    let content = format!(
        "---\ncategory: {}\nstatus: {}\ntags: [project, raios]\ncreated: {}\n---\n# {}\n\n## Overview\n{} is a project managed by R-AI-OS.\n\n## Details\n- Path: {}\n",
        project.category,
        project.status,
        chrono::Local::now().format("%Y-%m-%d"),
        project.name,
        project.name,
        project.local_path.display()
    );
    std::fs::write(vault_file, content)?;
    Ok(true)
}

pub fn probe_service_active(service: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", service])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn scan_extensions(dev_ops_path: &Path) -> Vec<ExtensionInfo> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        extension: ManifestMeta,
        #[serde(default)]
        commands: Vec<ManifestCmd>,
        #[serde(default)]
        config_schema: Vec<ManifestCfg>,
    }
    #[derive(serde::Deserialize)]
    struct ManifestMeta {
        name: String,
        version: String,
        description: String,
        services: Option<Vec<String>>,
    }
    #[derive(serde::Deserialize)]
    struct ManifestCmd {
        name: String,
        #[serde(default)]
        description: String,
    }
    #[derive(serde::Deserialize)]
    struct ManifestCfg {
        key: String,
        label: String,
        #[serde(rename = "type")]
        field_type: String,
        #[serde(default)]
        description: String,
    }

    let mut result = Vec::new();
    let categories = ["ai", "web", "tools", "embedded", "core", ""];
    for cat in categories {
        let search = if cat.is_empty() {
            dev_ops_path.to_path_buf()
        } else {
            dev_ops_path.join(cat)
        };
        let Ok(entries) = std::fs::read_dir(&search) else { continue };
        for entry in entries.flatten() {
            let proj = entry.path();
            let toml_path = proj.join("raios-extension.toml");
            if !toml_path.exists() {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&toml_path) else { continue };
            let Ok(m) = toml::from_str::<Manifest>(&raw) else { continue };
            let env_path = proj.join(".env");
            let services = m.extension.services.unwrap_or_default();

            let config_schema: Vec<ExtConfigField> = m
                .config_schema
                .iter()
                .map(|f| {
                    let value = read_env_key(&env_path, &f.key).unwrap_or_default();
                    ExtConfigField {
                        key: f.key.clone(),
                        label: f.label.clone(),
                        field_type: f.field_type.clone(),
                        description: f.description.clone(),
                        masked: f.field_type == "secret",
                        value,
                    }
                })
                .collect();

            result.push(ExtensionInfo {
                name: m.extension.name.clone(),
                version: m.extension.version.clone(),
                description: m.extension.description.clone(),
                path: proj,
                commands: m
                    .commands
                    .iter()
                    .map(|c| ExtCmdInfo {
                        name: c.name.clone(),
                        description: c.description.clone(),
                    })
                    .collect(),
                config_schema,
                services: services.clone(),
                service_statuses: services
                    .into_iter()
                    .map(|name| ExtServiceStatus {
                        active: probe_service_active(&name),
                        name,
                    })
                    .collect(),
            });
        }
    }
    result
}

pub fn run_extension_command_bg(
    tx: &std::sync::mpsc::Sender<BgMsg>,
    ext_path: &Path,
    toml_path: &Path,
    ext_name: &str,
    cmd_name: &str,
) {
    #[derive(serde::Deserialize)]
    struct Manifest {
        extension: Meta,
        #[serde(default)]
        commands: Vec<Cmd>,
    }
    #[derive(serde::Deserialize)]
    struct Meta {
        interpreter: Option<String>,
        entry: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Cmd {
        name: String,
        kind: String,
        #[serde(default)]
        args: Vec<String>,
        #[allow(dead_code)]
        env_key: Option<String>,
        #[allow(dead_code)]
        separator: Option<String>,
    }

    let raw = match std::fs::read_to_string(toml_path) {
        Ok(s) => s,
        Err(e) => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: format!("✗ Cannot read manifest: {}", e),
            })
            .ok();
            return;
        }
    };
    let manifest: Manifest = match toml::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: format!("✗ Manifest parse error: {}", e),
            })
            .ok();
            return;
        }
    };

    let def = match manifest.commands.iter().find(|c| c.name == cmd_name) {
        Some(d) => d,
        None => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: format!("✗ Unknown command: {}", cmd_name),
            })
            .ok();
            return;
        }
    };

    match def.kind.as_str() {
        "run" => {
            let rel_interp = manifest
                .extension
                .interpreter
                .as_deref()
                .unwrap_or("venv/bin/python");
            let interpreter = {
                let candidate = ext_path.join(rel_interp);
                if candidate.exists() {
                    candidate
                } else {
                    PathBuf::from("python3")
                }
            };
            let entry = manifest.extension.entry.as_deref().unwrap_or("main.py");
            let mut full_args = vec![entry.to_string()];
            full_args.extend_from_slice(&def.args);

            let child = Command::new(&interpreter)
                .args(&full_args)
                .current_dir(ext_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            match child {
                Err(e) => {
                    tx.send(BgMsg::ExtCmdOutput {
                        ext: ext_name.into(),
                        cmd: cmd_name.into(),
                        line: format!("✗ Spawn failed: {}", e),
                    })
                    .ok();
                }
                Ok(mut child) => {
                    if let Some(stdout) = child.stdout.take() {
                        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                            tx.send(BgMsg::ExtCmdOutput {
                                ext: ext_name.into(),
                                cmd: cmd_name.into(),
                                line,
                            })
                            .ok();
                        }
                    }
                    let code = child.wait().map(|s| s.code().unwrap_or(0)).unwrap_or(-1);
                    tx.send(BgMsg::ExtCmdOutput {
                        ext: ext_name.into(),
                        cmd: cmd_name.into(),
                        line: format!("✓ exited ({})", code),
                    })
                    .ok();
                }
            }
        }
        "service_start" | "service_stop" | "service_status" => {
            let action = match def.kind.as_str() {
                "service_start" => "start",
                "service_stop" => "stop",
                _ => "status",
            };
            #[derive(serde::Deserialize)]
            struct MetaFull {
                services: Option<Vec<String>>,
            }
            #[derive(serde::Deserialize)]
            struct ManifestFull {
                extension: MetaFull,
            }
            let services: Vec<String> = toml::from_str::<ManifestFull>(&raw)
                .ok()
                .and_then(|m| m.extension.services)
                .unwrap_or_default();
            for svc in &services {
                let out = Command::new("systemctl").args([action, svc]).output();
                let line = match out {
                    Ok(o) if o.status.success() => format!("✓ systemctl {} {}", action, svc),
                    Ok(o) => format!("✗ {}: {}", svc, String::from_utf8_lossy(&o.stderr).trim()),
                    Err(e) => format!("✗ systemctl error: {}", e),
                };
                tx.send(BgMsg::ExtCmdOutput {
                    ext: ext_name.into(),
                    cmd: cmd_name.into(),
                    line,
                })
                .ok();
            }
        }
        "env_append" => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: "Use: raios ext <name> follow <keyword> from terminal".into(),
            })
            .ok();
        }
        other => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: format!("Unhandled kind '{}' in TUI runner", other),
            })
            .ok();
        }
    }
}

pub fn read_env_key(env_path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(env_path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (k, v) = line.split_once('=')?;
        if k.trim() == key {
            return Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
        }
    }
    None
}

pub fn write_env_key(env_path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    let content = std::fs::read_to_string(env_path).unwrap_or_default();
    let mut found = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| {
            if !line.starts_with('#') && line.contains('=') {
                if let Some((k, _)) = line.split_once('=') {
                    if k.trim() == key {
                        found = true;
                        return format!("{}={}", key, value);
                    }
                }
            }
            line.to_string()
        })
        .collect();
    if !found {
        lines.push(format!("{}={}", key, value));
    }
    std::fs::write(env_path, lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_panel_data_defaults_to_empty() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let data = load_inbox_panel_data_from_conn(&conn).unwrap();
        assert!(data.approvals.is_empty());
        assert!(data.runs.is_empty());
        assert!(data.blocked.is_empty());
    }

    #[test]
    fn policies_panel_data_without_db_is_still_constructible() {
        let data = load_policies_panel_data();
        let _ = data.policy;
    }

    #[test]
    fn sort_project_indices_by_name_is_stable_for_simple_case() {
        let projects = vec![
            EntityProject {
                name: "zeta".into(),
                category: "tools".into(),
                local_path: PathBuf::from("/tmp/zeta"),
                github: None,
                status: "active".into(),
                stars: None,
                last_commit: None,
                version: None,
                version_nickname: None,
            },
            EntityProject {
                name: "alpha".into(),
                category: "tools".into(),
                local_path: PathBuf::from("/tmp/alpha"),
                github: None,
                status: "active".into(),
                stars: None,
                last_commit: None,
                version: None,
                version_nickname: None,
            },
        ];

        let indices = sort_project_indices(&projects, &[], &SortMode::Name);
        assert_eq!(indices, vec![1, 0]);
    }

    #[test]
    fn project_detail_data_without_memory_file_is_empty_but_valid() {
        let dir = tempfile::tempdir().unwrap();
        let data = load_project_detail_data(dir.path());
        assert_eq!(data.memory_lines.first().map(String::as_str), Some("# Error"));
    }
}
