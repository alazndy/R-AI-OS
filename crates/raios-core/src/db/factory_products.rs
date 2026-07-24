//! Repository boundary for canonical Product Factory products and plans.

use rusqlite::{params, Connection, OptionalExtension, Result, Transaction};
use uuid::Uuid;

pub const FACTORY_PRODUCTS_TABLE: &str = "cp_factory_products";
pub const FACTORY_WORKSPACES_TABLE: &str = "cp_workspaces";
pub const FACTORY_PLANS_TABLE: &str = "cp_plans";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryWorkspaceCreated {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryProductCreated {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
}

pub fn set_factory_product_mode(
    tx: &Transaction<'_>,
    owner_subject: &str,
    product_id: &str,
    mode: &str,
) -> Result<bool> {
    let changed = tx.execute(
        "UPDATE cp_factory_products SET factory_mode = ?1, updated_at = datetime('now','utc')
         WHERE id = ?2 AND owner_subject = ?3",
        params![mode, product_id, owner_subject],
    )?;
    Ok(changed == 1)
}

pub fn load_factory_product_mode(tx: &Transaction<'_>, product_id: &str) -> Result<String> {
    tx.query_row(
        "SELECT factory_mode FROM cp_factory_products WHERE id = ?1",
        [product_id],
        |row| row.get(0),
    )
}

pub fn load_factory_product_scaffold_context(
    tx: &Transaction<'_>,
    owner_subject: &str,
    product_id: &str,
) -> Result<Option<(String, String, String, String)>> {
    tx.query_row(
        "SELECT product.title, product.project_path, COALESCE((SELECT item.response_text FROM cp_factory_intake_items item JOIN cp_factory_intake_sessions session ON session.id=item.session_id WHERE session.product_id=product.id AND item.question_key='first_platform' ORDER BY session.created_at DESC LIMIT 1), ''), product.factory_mode FROM cp_factory_products product WHERE product.id=?1 AND product.owner_subject=?2",
        params![product_id, owner_subject], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).optional()
}

pub fn save_factory_product_project_path(
    tx: &Transaction<'_>,
    owner_subject: &str,
    product_id: &str,
    project_path: &str,
) -> Result<bool> {
    Ok(tx.execute("UPDATE cp_factory_products SET project_path=?1, updated_at=datetime('now','utc') WHERE id=?2 AND owner_subject=?3", params![project_path, product_id, owner_subject])? == 1)
}

/// Persist a verified local Git worktree binding. Source metadata is captured
/// by the runtime before this transaction begins; this repository boundary
/// remains responsible only for owner-bound persistence.
pub fn attach_factory_product_existing_project(
    tx: &Transaction<'_>,
    owner_subject: &str,
    product_id: &str,
    project_path: &str,
    source_remote: &str,
    source_revision: &str,
) -> Result<bool> {
    Ok(tx.execute(
        "UPDATE cp_factory_products
         SET project_path=?1, source_remote=?2, source_revision=?3, updated_at=datetime('now','utc')
         WHERE id=?4 AND owner_subject=?5",
        params![
            project_path,
            source_remote,
            source_revision,
            product_id,
            owner_subject
        ],
    )? == 1)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryPlanCreated {
    pub id: String,
    pub product_id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FactoryProductOverviewRow {
    pub id: String,
    pub title: String,
    pub status: String,
    pub mode: String,
    pub project_path: Option<String>,
    pub source_remote: Option<String>,
    pub source_revision: Option<String>,
    pub stack: Option<String>,
    pub scaffold_state: String,
    pub quality_blockers: u32,
    pub release_blockers: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FactoryOverviewRow {
    pub product_count: u32,
    pub active_cycle_count: u32,
    pub pending_change_request_count: u32,
    pub open_support_item_count: u32,
    pub blocking_quality_profile_count: u32,
    pub release_draft_count: u32,
    pub completed_verify_stage_count: u32,
    pub approved_closed_testing_release_count: u32,
    pub latest_product: Option<FactoryProductOverviewRow>,
}

/// Read-only skeleton probe used by architecture tests and future repositories.
pub fn factory_products_schema_available(conn: &Connection) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [FACTORY_PRODUCTS_TABLE],
        |row| row.get(0),
    )
}

pub fn create_factory_workspace(
    tx: &Transaction<'_>,
    owner_subject: &str,
    name: &str,
) -> Result<FactoryWorkspaceCreated> {
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_workspaces (id, name, owner_subject, status) VALUES (?1, ?2, ?3, 'draft')",
        params![id, name, owner_subject],
    )?;
    Ok(FactoryWorkspaceCreated {
        id,
        name: name.into(),
    })
}

pub fn create_factory_product_draft(
    tx: &Transaction<'_>,
    owner_subject: &str,
    workspace_id: &str,
    title: &str,
) -> Result<Option<FactoryProductCreated>> {
    let owns_workspace: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_workspaces WHERE id = ?1 AND owner_subject = ?2)",
        params![workspace_id, owner_subject],
        |row| row.get(0),
    )?;
    if !owns_workspace {
        return Ok(None);
    }

    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_factory_products (id, workspace_id, owner_subject, title, status)
         VALUES (?1, ?2, ?3, ?4, 'draft')",
        params![id, workspace_id, owner_subject, title],
    )?;
    Ok(Some(FactoryProductCreated {
        id,
        workspace_id: workspace_id.into(),
        title: title.into(),
    }))
}

pub fn create_factory_plan_draft(
    tx: &Transaction<'_>,
    owner_subject: &str,
    product_id: &str,
    title: &str,
) -> Result<Option<FactoryPlanCreated>> {
    let workspace_id: Option<String> = tx
        .query_row(
            "SELECT workspace_id FROM cp_factory_products WHERE id = ?1 AND owner_subject = ?2",
            params![product_id, owner_subject],
            |row| row.get(0),
        )
        .optional()?;
    let Some(workspace_id) = workspace_id else {
        return Ok(None);
    };
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_plans (id, workspace_id, product_id, title, status) VALUES (?1, ?2, ?3, ?4, 'planned')",
        params![id, workspace_id, product_id, title],
    )?;
    Ok(Some(FactoryPlanCreated {
        id,
        product_id: product_id.into(),
        title: title.into(),
    }))
}

pub fn approve_factory_plan(
    tx: &Transaction<'_>,
    owner_subject: &str,
    plan_id: &str,
) -> Result<bool> {
    let changed = tx.execute(
        "UPDATE cp_plans SET status = 'approved', updated_at = datetime('now','utc')
         WHERE id = ?1 AND status = 'planned' AND product_id IN
         (SELECT id FROM cp_factory_products WHERE owner_subject = ?2)",
        params![plan_id, owner_subject],
    )?;
    Ok(changed == 1)
}

pub fn factory_product_owned_by(
    conn: &Connection,
    product_id: &str,
    owner_subject: &str,
) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_factory_products WHERE id = ?1 AND owner_subject = ?2)",
        params![product_id, owner_subject],
        |row| row.get(0),
    )
}

pub fn load_factory_product_title_for_owner(
    tx: &Transaction<'_>,
    product_id: &str,
    owner_subject: &str,
) -> Result<Option<String>> {
    tx.query_row(
        "SELECT title FROM cp_factory_products WHERE id = ?1 AND owner_subject = ?2",
        params![product_id, owner_subject],
        |row| row.get(0),
    )
    .optional()
}

/// Canonical, read-only Factory projection for control surfaces.
///
/// This function never opens a transaction and never derives lifecycle state
/// from free-form task text. Later mutations must use separately reviewed
/// domain repositories and approval paths.
pub fn load_factory_overview(conn: &Connection) -> Result<FactoryOverviewRow> {
    let product_count = count(conn, "SELECT COUNT(*) FROM cp_factory_products")?;
    let active_cycle_count = count(
        conn,
        "SELECT COUNT(*) FROM cp_factory_cycles WHERE status IN ('planned', 'active', 'blocked')",
    )?;
    let pending_change_request_count = count(
        conn,
        "SELECT COUNT(*) FROM cp_factory_change_requests WHERE status IN ('proposed', 'assessing', 'awaiting_approval')",
    )?;
    let open_support_item_count = count(
        conn,
        "SELECT COUNT(*) FROM cp_factory_support_items WHERE status IN ('new', 'triaged', 'in_progress')",
    )?;
    let blocking_quality_profile_count = count(conn, "SELECT COUNT(*) FROM cp_factory_quality_profiles profile WHERE profile.required=1 AND NOT EXISTS (SELECT 1 FROM cp_factory_quality_checks check_row WHERE check_row.profile_id=profile.id AND check_row.status='passed' AND check_row.evidence_ref IS NOT NULL AND trim(check_row.evidence_ref)<>'')")?;
    let release_draft_count = count(
        conn,
        "SELECT COUNT(*) FROM cp_factory_releases WHERE status='draft'",
    )?;
    let completed_verify_stage_count = count(
        conn,
        "SELECT COUNT(*) FROM cp_factory_stage_runs WHERE stage='verify' AND status='completed'",
    )?;
    let approved_closed_testing_release_count = count(
        conn,
        "SELECT COUNT(*) FROM cp_factory_release_channels WHERE channel_kind='closed_testing' AND status='approved'",
    )?;
    let latest_product = conn
        .query_row(
            "SELECT id, title, status, factory_mode, project_path, source_remote, source_revision FROM cp_factory_products ORDER BY updated_at DESC, id DESC LIMIT 1",
            [],
            |row| {
                let id: String = row.get(0)?;
                let title: String = row.get(1)?;
                let status: String = row.get(2)?;
                let mode: String = row.get(3)?;
                let project_path_str: String = row.get(4)?;
                let source_remote_str: String = row.get(5)?;
                let source_revision_str: String = row.get(6)?;
                let project_path = if project_path_str.trim().is_empty() { None } else { Some(project_path_str.clone()) };
                let source_remote = if source_remote_str.trim().is_empty() { None } else { Some(source_remote_str) };
                let source_revision = if source_revision_str.trim().is_empty() { None } else { Some(source_revision_str) };
                let scaffold_state = if source_remote.is_some() && source_revision.is_some() {
                    "attached".to_string()
                } else if project_path.is_some() {
                    "scaffolded".to_string()
                } else {
                    "unscaffolded".to_string()
                };

                let stack = project_path.as_ref().and_then(|p| {
                    let path = std::path::Path::new(p);
                    if path.join("Cargo.toml").exists() { Some("Rust".into()) }
                    else if path.join("pubspec.yaml").exists() { Some("Flutter".into()) }
                    else if path.join("package.json").exists() {
                        let content = std::fs::read_to_string(path.join("package.json")).unwrap_or_default();
                        if content.contains("react-native") || content.contains("expo") { Some("React Native / Expo".into()) }
                        else { Some("Web".into()) }
                    } else { None }
                });

                let quality_blockers: u32 = conn.query_row(
                    "SELECT COUNT(*) FROM cp_factory_quality_profiles profile WHERE profile.product_id=?1 AND profile.required=1 AND NOT EXISTS (SELECT 1 FROM cp_factory_quality_checks check_row WHERE check_row.profile_id=profile.id AND check_row.status='passed' AND check_row.evidence_ref IS NOT NULL AND trim(check_row.evidence_ref)<>'')",
                    [&id],
                    |r| r.get(0),
                ).unwrap_or(0);

                let release_blockers: u32 = conn.query_row(
                    "SELECT COUNT(*) FROM cp_factory_releases WHERE product_id=?1 AND status='draft'",
                    [&id],
                    |r| r.get(0),
                ).unwrap_or(0);

                Ok(FactoryProductOverviewRow {
                    id,
                    title,
                    status,
                    mode,
                    project_path,
                    source_remote,
                    source_revision,
                    stack,
                    scaffold_state,
                    quality_blockers,
                    release_blockers,
                })
            },
        )
        .optional()?;

    Ok(FactoryOverviewRow {
        product_count,
        active_cycle_count,
        pending_change_request_count,
        open_support_item_count,
        blocking_quality_profile_count,
        release_draft_count,
        completed_verify_stage_count,
        approved_closed_testing_release_count,
        latest_product,
    })
}

fn count(conn: &Connection, query: &str) -> Result<u32> {
    let value: i64 = conn.query_row(query, [], |row| row.get(0))?;
    Ok(u32::try_from(value).unwrap_or(u32::MAX))
}
