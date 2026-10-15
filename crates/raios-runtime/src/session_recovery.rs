//! Global native-conversation inventory and exact-ID continuation. No transcript
//! bodies are copied into RAIOS memory, and no "latest session" guess is made.
use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const FILE_LIMIT: usize = 20_000;
const HEADER_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone)]
pub struct RecoveryRoots {
    pub codex: PathBuf,
    pub claude: PathBuf,
    pub opencode: PathBuf,
}
impl RecoveryRoots {
    pub fn live() -> Result<Self> {
        let home = dirs::home_dir().context("cannot resolve home directory")?;
        Ok(Self {
            codex: std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex")),
            claude: std::env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude")),
            opencode: dirs::data_dir()
                .unwrap_or_else(|| home.join(".local/share"))
                .join("opencode/opencode.db"),
        })
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct SavedSession {
    pub key: String,
    pub agent: String,
    pub native_session_id: Option<String>,
    pub project_path: String,
    pub name: String,
    pub work: Option<String>,
    pub updated_at: String,
    pub wrapper_run_id: Option<String>,
    pub availability: String,
    pub resumable: bool,
}
#[derive(Debug, Serialize)]
pub struct RecoveryCatalog {
    pub sessions: Vec<SavedSession>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct ResumePlan {
    pub agent: String,
    pub native_session_id: String,
    pub project_path: String,
    pub name: String,
    pub work: Option<String>,
    pub args: Vec<String>,
}

fn display_text(text: &str, limit: usize) -> String {
    raios_core::security::redact_secrets(text)
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}
fn valid_native_id(agent: &str, id: &str) -> bool {
    match agent {
        "codex" | "claude" => uuid::Uuid::parse_str(id).is_ok(),
        "opencode" => {
            id.starts_with("ses_")
                && id.len() > 4
                && id.len() <= 128
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }
        _ => false,
    }
}
fn native_row(agent: &str, id: &str, cwd: &str, name: &str, updated: &str) -> Option<SavedSession> {
    if !valid_native_id(agent, id)
        || !Path::new(cwd).is_absolute()
        || cwd.chars().any(char::is_control)
    {
        return None;
    }
    Some(SavedSession {
        key: format!("{agent}:{id}"),
        agent: agent.into(),
        native_session_id: Some(id.into()),
        project_path: cwd.into(),
        name: {
            let title = display_text(name, 120);
            if title.trim().is_empty() {
                format!("{agent} {}", id.chars().take(8).collect::<String>())
            } else {
                title
            }
        },
        work: None,
        updated_at: updated.into(),
        wrapper_run_id: None,
        availability: if Path::new(cwd).is_dir() {
            "saved"
        } else {
            "project_missing"
        }
        .into(),
        resumable: Path::new(cwd).is_dir(),
    })
}

fn scan_jsonl(
    root: &Path,
    agent: &str,
    depth: usize,
    rows: &mut BTreeMap<String, SavedSession>,
    warnings: &mut Vec<String>,
) {
    if !root.exists() {
        return;
    }
    if root
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(true)
    {
        warnings.push(format!("{agent}: history root is not a regular directory"));
        return;
    }
    let mut seen = 0;
    for (visited, entry) in WalkDir::new(root)
        .max_depth(depth)
        .follow_links(false)
        .into_iter()
        .enumerate()
    {
        if visited > FILE_LIMIT * 2 {
            warnings.push(format!(
                "{agent}: traversal limit reached; inventory is incomplete"
            ));
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                warnings.push(format!("{agent}: a history directory could not be read"));
                continue;
            }
        };
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|x| x.to_str()) != Some("jsonl")
        {
            continue;
        }
        seen += 1;
        if seen > FILE_LIMIT {
            warnings.push(format!(
                "{agent}: history scan limit reached; inventory is incomplete"
            ));
            break;
        }
        let Ok(file) = crate::anka::provenance::open_regular_file(entry.path()) else {
            continue;
        };
        let modified = file
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
        let mut bytes = Vec::new();
        if file.take(HEADER_BYTES).read_to_end(&mut bytes).is_err() {
            continue;
        }
        let header = String::from_utf8_lossy(&bytes);
        for line in header.lines().take(64) {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let (id, cwd, name, updated) = if agent == "codex" {
                if v["type"] != "session_meta" {
                    continue;
                }
                let p = &v["payload"];
                if p["source"].is_object() || p["source"] == "subagent" {
                    break;
                }
                (
                    p["id"].as_str(),
                    p["cwd"].as_str(),
                    None,
                    p["timestamp"].as_str(),
                )
            } else {
                if v["isSidechain"] == true {
                    break;
                }
                (
                    v["sessionId"].as_str(),
                    v["cwd"].as_str(),
                    v["customTitle"].as_str(),
                    v["timestamp"].as_str(),
                )
            };
            let (Some(id), Some(cwd)) = (id, cwd) else {
                continue;
            };
            // IDs must match the actual native store filename, not an embedded forged identifier.
            let stem = entry
                .path()
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("");
            if (agent == "claude" && stem != id)
                || (agent == "codex" && !stem.ends_with(&format!("-{id}")))
            {
                break;
            }
            let title = name
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{agent} {}", id.chars().take(8).collect::<String>()));
            if let Some(row) = native_row(
                agent,
                id,
                cwd,
                &title,
                modified.as_deref().or(updated).unwrap_or(""),
            ) {
                rows.entry(row.key.clone()).or_insert(row);
            }
            break;
        }
    }
}
fn codex_names(root: &Path, rows: &mut BTreeMap<String, SavedSession>) {
    let Ok(file) = crate::anka::provenance::open_regular_file(&root.join("session_index.jsonl"))
    else {
        return;
    };
    // A bounded index pass; this contains native names, never imported conversation bodies.
    let mut bytes = Vec::new();
    if file.take(8 * 1024 * 1024).read_to_end(&mut bytes).is_err() {
        return;
    }
    for line in String::from_utf8_lossy(&bytes).lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(id) = v["id"].as_str() else {
            continue;
        };
        if let Some(row) = rows.get_mut(&format!("codex:{id}")) {
            if let Some(name) = v["thread_name"].as_str() {
                let name = display_text(name, 120);
                if !name.trim().is_empty() {
                    row.name = name;
                }
            }
            if let Some(t) = v["updated_at"].as_str() {
                row.updated_at = t.into();
            }
        }
    }
}
fn scan_opencode(
    path: &Path,
    rows: &mut BTreeMap<String, SavedSession>,
    warnings: &mut Vec<String>,
) {
    if !path.exists() {
        return;
    }
    if crate::anka::provenance::open_regular_file(path).is_err() {
        warnings.push("opencode: native database is not a readable regular file".into());
        return;
    }
    let result = (|| -> Result<()> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(std::time::Duration::from_millis(250))?;
        let mut stmt=conn.prepare("SELECT id,directory,title,time_updated FROM session WHERE parent_id IS NULL AND time_archived IS NULL ORDER BY time_updated DESC LIMIT 20001")?;
        let found = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for (n, row) in found.enumerate() {
            if n >= FILE_LIMIT {
                warnings.push("opencode: inventory limit reached".into());
                break;
            }
            let (id, cwd, title, t) = row?;
            let t = chrono::DateTime::from_timestamp_millis(t)
                .map(|t| t.to_rfc3339())
                .unwrap_or_default();
            if let Some(row) = native_row("opencode", &id, &cwd, &title, &t) {
                rows.insert(row.key.clone(), row);
            }
        }
        Ok(())
    })();
    if result.is_err() {
        warnings.push("opencode: native database unavailable or unsupported schema".into());
    }
}

/// Read-only inventory: reconstructs native sessions even when no wrapper shutdown ran.
pub fn catalog(roots: &RecoveryRoots, db_path: &Path) -> Result<RecoveryCatalog> {
    let mut rows = BTreeMap::new();
    let mut warnings = Vec::new();
    scan_jsonl(
        &roots.codex.join("sessions"),
        "codex",
        4,
        &mut rows,
        &mut warnings,
    );
    scan_jsonl(
        &roots.claude.join("projects"),
        "claude",
        2,
        &mut rows,
        &mut warnings,
    );
    codex_names(&roots.codex, &mut rows);
    scan_opencode(&roots.opencode, &mut rows, &mut warnings);
    if db_path.exists() {
        match Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(conn) => {
                let dead: HashSet<String> =
                    crate::wrapper_liveness::reconcile_sessions(db_path, false, false)
                        .map(|r| r.candidates.into_iter().map(|c| c.run_id).collect())
                        .unwrap_or_default();
                // Legacy versions do not have cp_wrapper_resume; never migrate on inventory reads.
                let has_resume = conn
                    .prepare("SELECT agent_run_id FROM cp_wrapper_resume LIMIT 0")
                    .is_ok();
                let sql = if has_resume {
                    "SELECT r.id,r.agent_name,r.status,t.title,t.description,r.started_at,j.workspace_root,j.native_session_id FROM cp_agent_runs r JOIN cp_tasks t ON t.id=r.task_id JOIN cp_wrapper_resume j ON j.agent_run_id=r.id WHERE r.provider='wrapper' ORDER BY r.started_at DESC,r.id DESC LIMIT 20001"
                } else {
                    "SELECT r.id,r.agent_name,r.status,t.title,t.description,r.started_at,c.workspace_root,NULL FROM cp_agent_runs r JOIN cp_tasks t ON t.id=r.task_id JOIN cp_run_contracts c ON c.id=r.run_contract_id WHERE r.provider='wrapper' ORDER BY r.started_at DESC,r.id DESC LIMIT 20001"
                };
                let result = (|| -> Result<()> {
                    let mut stmt = conn.prepare(sql)?;
                    let found = stmt.query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, Option<String>>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, Option<String>>(7)?,
                        ))
                    })?;
                    for (n, row) in found.enumerate() {
                        if n >= FILE_LIMIT {
                            warnings.push("wrapper: inventory limit reached".into());
                            break;
                        }
                        let (run, identity, status, name, work, t, cwd, native) = row?;
                        let agent = match identity.as_str() {
                            "claude_kaira" => "claude",
                            "codex_kaira" => "codex",
                            "opencode_kaira" => "opencode",
                            "antigravity_kaira" => "agy",
                            other => other,
                        };
                        let key = native.as_ref().map(|id| format!("{agent}:{id}"));
                        if let Some(row) = key.as_ref().and_then(|k| rows.get_mut(k)) {
                            let native_path = Path::new(&row.project_path)
                                .canonicalize()
                                .unwrap_or_else(|_| PathBuf::from(&row.project_path));
                            let wrapper_path = Path::new(&cwd)
                                .canonicalize()
                                .unwrap_or_else(|_| PathBuf::from(&cwd));
                            if row.wrapper_run_id.is_none() && native_path == wrapper_path {
                                row.name = display_text(&name, 120);
                                row.work = Some(display_text(&work, 2000))
                                    .filter(|s| !s.trim().is_empty());
                                row.wrapper_run_id = Some(run.clone());
                            }
                            if status == "running" && !dead.contains(&run) {
                                row.availability = "running_or_unverified".into();
                                row.resumable = false;
                            }
                        } else {
                            rows.insert(
                                format!("wrapper:{run}"),
                                SavedSession {
                                    key: format!("wrapper:{run}"),
                                    agent: agent.into(),
                                    native_session_id: native,
                                    project_path: cwd,
                                    name: display_text(&name, 120),
                                    work: Some(display_text(&work, 2000)),
                                    updated_at: t.unwrap_or_default(),
                                    wrapper_run_id: Some(run),
                                    availability: "native_conversation_unavailable".into(),
                                    resumable: false,
                                },
                            );
                        }
                    }
                    Ok(())
                })();
                if result.is_err() {
                    warnings.push("wrapper: saved launch metadata could not be read".into());
                }
            }
            Err(_) => warnings.push(
                "wrapper: control-plane database unavailable; native inventory remains available"
                    .into(),
            ),
        }
    }
    let mut sessions: Vec<_> = rows.into_values().collect();
    sessions.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.key.cmp(&b.key))
    });
    Ok(RecoveryCatalog { sessions, warnings })
}

pub fn plan(catalog: &RecoveryCatalog, key: &str) -> Result<ResumePlan> {
    let found: Vec<_> = catalog
        .sessions
        .iter()
        .filter(|s| {
            s.key == key
                || s.native_session_id.as_deref() == Some(key)
                || s.wrapper_run_id.as_deref() == Some(key)
        })
        .collect();
    if found.len() != 1 {
        bail!(
            "session not found or ambiguous; select the full agent:id key from raios resume --list"
        );
    }
    let row = found[0];
    if !row.resumable {
        bail!(
            "session cannot safely resume: {}; select an available native conversation",
            row.availability
        );
    }
    if !Path::new(&row.project_path).is_dir() {
        bail!("saved project directory is missing");
    }
    let id = row
        .native_session_id
        .as_deref()
        .context("missing native conversation ID")?;
    if !valid_native_id(&row.agent, id) {
        bail!("unsupported native session ID");
    }
    let args = match row.agent.as_str() {
        "codex" => vec!["resume".into(), id.into()],
        "claude" => vec!["--resume".into(), id.into()],
        "opencode" => vec!["--session".into(), id.into()],
        _ => bail!("agent does not support verified native continuation"),
    };
    Ok(ResumePlan {
        agent: row.agent.clone(),
        native_session_id: id.into(),
        project_path: row.project_path.clone(),
        name: row.name.clone(),
        work: row.work.clone(),
        args,
    })
}

/// Called before spawn. Claude permits a generated ID; other agents keep their
/// native automatic persistence and are discoverable globally after a crash.
pub fn prepare_native_checkpoint(agent: &str, extra: &mut Vec<String>) -> Option<String> {
    let agent = agent.to_ascii_lowercase();
    if extra
        .iter()
        .any(|a| matches!(a.as_str(), "--fork-session" | "--no-session-persistence"))
    {
        return None;
    }
    let flags = match agent.as_str() {
        "claude" => vec!["--resume", "-r", "--session-id"],
        "opencode" => vec!["--session", "-s"],
        _ => vec![],
    };
    let explicit = if agent == "codex" && extra.first().map(String::as_str) == Some("resume") {
        extra.get(1).cloned()
    } else {
        extra.iter().enumerate().find_map(|(n, arg)| {
            flags.iter().find_map(|flag| {
                if arg == flag {
                    extra.get(n + 1).cloned()
                } else {
                    arg.strip_prefix(&format!("{flag}=")).map(str::to_owned)
                }
            })
        })
    };
    if let Some(id) = explicit.filter(|id| valid_native_id(&agent, id)) {
        return Some(id);
    }
    if agent == "claude" {
        if extra.iter().any(|a| {
            a == "--continue"
                || a == "-c"
                || flags
                    .iter()
                    .any(|flag| a == flag || a.starts_with(&format!("{flag}=")))
        }) {
            return None;
        }
        let id = uuid::Uuid::new_v4().to_string();
        extra.splice(0..0, ["--session-id".to_owned(), id.clone()]);
        return Some(id);
    }
    None
}

#[cfg(test)]
#[path = "session_recovery_tests.rs"]
mod tests;
