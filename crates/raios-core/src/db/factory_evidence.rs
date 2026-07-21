//! Repository boundary for evidence and quality records.

use rusqlite::{params, OptionalExtension, Result, Transaction};
use uuid::Uuid;

pub const FACTORY_EVIDENCE_LINKS_TABLE: &str = "cp_factory_evidence_links";
pub const FACTORY_EVIDENCE_DEPENDENCIES_TABLE: &str = "cp_factory_evidence_dependencies";
pub const FACTORY_QUALITY_PROFILES_TABLE: &str = "cp_factory_quality_profiles";
pub const FACTORY_QUALITY_CHECKS_TABLE: &str = "cp_factory_quality_checks";

/// Required evidence gates for the Expo/React Native pilot through actual
/// Android and iOS closed-testing readiness.
pub const REACT_NATIVE_CLOSED_TESTING_QUALITY_GATES: [&str; 6] = [
    "TypeScript verification",
    "Expo public configuration",
    "Web production export",
    "High/critical dependency audit",
    "Android device or closed-testing evidence",
    "iOS device or TestFlight evidence",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryReleaseReadiness {
    pub required_quality_blockers: u32,
    pub completed_verify_stage: bool,
    pub pending_impact_assessments: u32,
    pub stale_evidence_count: u32,
    pub ready: bool,
}

/// Record an explicit requirement dependency for existing owner-bound stage
/// evidence. Later requirement revisions invalidate only these linked records.
pub fn link_factory_stage_evidence_to_requirement(
    tx: &Transaction<'_>,
    owner: &str,
    evidence_id: &str,
    requirement_id: &str,
) -> Result<bool> {
    let same_product: bool = tx.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM cp_factory_evidence_links evidence
           JOIN cp_factory_requirements requirement ON requirement.product_id=evidence.product_id
           JOIN cp_factory_products product ON product.id=evidence.product_id
           WHERE evidence.id=?1 AND evidence.subject_kind='stage_run'
             AND requirement.id=?2 AND product.owner_subject=?3
         )",
        params![evidence_id, requirement_id, owner],
        |row| row.get(0),
    )?;
    if !same_product {
        return Ok(false);
    }
    tx.execute(
        "INSERT OR IGNORE INTO cp_factory_evidence_dependencies
         (id, evidence_id, target_kind, target_id, relation_kind)
         VALUES (?1,?2,'requirement',?3,'verifies')",
        params![Uuid::new_v4().to_string(), evidence_id, requirement_id],
    )?;
    Ok(true)
}

pub fn record_factory_stage_evidence(
    tx: &Transaction<'_>,
    owner: &str,
    cycle_id: &str,
    stage: &str,
    content_ref: &str,
) -> Result<Option<String>> {
    if content_ref.trim().is_empty() {
        return Ok(None);
    }
    let stage_run_id: Option<String> = tx.query_row(
        "SELECT run.id FROM cp_factory_stage_runs run JOIN cp_factory_cycles cycle ON cycle.id=run.cycle_id JOIN cp_factory_products product ON product.id=cycle.product_id WHERE run.cycle_id=?1 AND run.stage=?2 AND product.owner_subject=?3",
        params![cycle_id, stage, owner], |row| row.get(0),
    ).optional()?;
    let Some(stage_run_id) = stage_run_id else {
        return Ok(None);
    };
    let id = Uuid::new_v4().to_string();
    tx.execute("INSERT INTO cp_factory_evidence_links (id, product_id, subject_kind, subject_id, content_ref, storage_class) SELECT ?1, cycle.product_id, 'stage_run', ?2, ?3, 'inline_reference' FROM cp_factory_cycles cycle WHERE cycle.id=?4", params![id, stage_run_id, content_ref, cycle_id])?;
    Ok(Some(id))
}

/// Link an existing canonical artifact to an owned Factory stage. The artifact
/// must belong to that stage's task-graph node and expose a non-empty digest or
/// other immutable content reference; artifact bytes remain outside SQLite.
pub fn record_factory_stage_artifact_evidence(
    tx: &Transaction<'_>,
    owner: &str,
    cycle_id: &str,
    stage: &str,
    artifact_id: &str,
) -> Result<Option<String>> {
    let artifact_ref: Option<(String, String)> = tx
        .query_row(
            "SELECT artifact.content_ref, run.id
             FROM cp_factory_stage_runs run
             JOIN cp_factory_cycles cycle ON cycle.id=run.cycle_id
             JOIN cp_factory_products product ON product.id=cycle.product_id
             JOIN cp_task_graph_nodes node ON node.graph_id=run.task_graph_id AND node.node_id=run.stage
             JOIN cp_artifacts artifact ON artifact.task_id=node.task_id
             WHERE run.cycle_id=?1 AND run.stage=?2 AND artifact.id=?3
               AND product.owner_subject=?4
               AND artifact.content_ref IS NOT NULL AND trim(artifact.content_ref)<>''",
            params![cycle_id, stage, artifact_id, owner],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((content_ref, stage_run_id)) = artifact_ref else {
        return Ok(None);
    };
    let evidence_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_factory_evidence_links
         (id, product_id, subject_kind, subject_id, artifact_id, content_ref, storage_class)
         SELECT ?1, cycle.product_id, 'stage_run', ?2, ?3, ?4, 'content_addressed_artifact'
         FROM cp_factory_cycles cycle WHERE cycle.id=?5",
        params![
            evidence_id,
            stage_run_id,
            artifact_id,
            content_ref,
            cycle_id
        ],
    )?;
    Ok(Some(evidence_id))
}

pub fn create_factory_quality_profile(
    tx: &Transaction<'_>,
    owner: &str,
    product_id: &str,
    name: &str,
    required: bool,
) -> Result<Option<String>> {
    let owned: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_factory_products WHERE id=?1 AND owner_subject=?2)",
        params![product_id, owner],
        |row| row.get(0),
    )?;
    if !owned {
        return Ok(None);
    }
    let id = Uuid::new_v4().to_string();
    tx.execute("INSERT INTO cp_factory_quality_profiles (id, product_id, name, required) VALUES (?1,?2,?3,?4)", params![id, product_id, name, required])?;
    Ok(Some(id))
}

/// Seed the reviewed React Native closed-testing profile without duplicating
/// existing gates. Passing checks remain explicit owner-recorded evidence.
pub fn ensure_react_native_closed_testing_quality_profile(
    tx: &Transaction<'_>,
    owner: &str,
    product_id: &str,
) -> Result<Option<Vec<String>>> {
    let owned: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_factory_products WHERE id=?1 AND owner_subject=?2)",
        params![product_id, owner],
        |row| row.get(0),
    )?;
    if !owned {
        return Ok(None);
    }
    let mut profile_ids = Vec::with_capacity(REACT_NATIVE_CLOSED_TESTING_QUALITY_GATES.len());
    for name in REACT_NATIVE_CLOSED_TESTING_QUALITY_GATES {
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM cp_factory_quality_profiles WHERE product_id=?1 AND name=?2 ORDER BY created_at ASC LIMIT 1",
                params![product_id, name],
                |row| row.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO cp_factory_quality_profiles (id, product_id, name, required) VALUES (?1,?2,?3,1)",
                    params![id, product_id, name],
                )?;
                id
            }
        };
        profile_ids.push(id);
    }
    Ok(Some(profile_ids))
}

pub fn record_factory_quality_check(
    tx: &Transaction<'_>,
    owner: &str,
    profile_id: &str,
    passed: bool,
    evidence_ref: &str,
) -> Result<Option<String>> {
    if evidence_ref.trim().is_empty() {
        return Ok(None);
    }
    let product_id: Option<String> = tx.query_row(
        "SELECT profile.product_id FROM cp_factory_quality_profiles profile JOIN cp_factory_products product ON product.id=profile.product_id WHERE profile.id=?1 AND product.owner_subject=?2",
        params![profile_id, owner], |row| row.get(0),
    ).optional()?;
    let Some(product_id) = product_id else {
        return Ok(None);
    };
    let id = Uuid::new_v4().to_string();
    let status = if passed { "passed" } else { "failed" };
    tx.execute("INSERT INTO cp_factory_quality_checks (id, profile_id, product_id, status, evidence_ref, checked_at) VALUES (?1,?2,?3,?4,?5,datetime('now','utc'))", params![id, profile_id, product_id, status, evidence_ref])?;
    Ok(Some(id))
}

pub fn factory_release_ready(
    tx: &Transaction<'_>,
    owner: &str,
    product_id: &str,
) -> Result<Option<bool>> {
    Ok(factory_release_readiness(tx, owner, product_id)?.map(|readiness| readiness.ready))
}

pub fn factory_release_readiness(
    tx: &Transaction<'_>,
    owner: &str,
    product_id: &str,
) -> Result<Option<FactoryReleaseReadiness>> {
    let owned: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_factory_products WHERE id=?1 AND owner_subject=?2)",
        params![product_id, owner],
        |row| row.get(0),
    )?;
    if !owned {
        return Ok(None);
    }
    let blockers: u32 = tx.query_row(
        "SELECT COUNT(*) FROM cp_factory_quality_profiles profile WHERE profile.product_id=?1 AND profile.required=1 AND NOT EXISTS (SELECT 1 FROM cp_factory_quality_checks check_row WHERE check_row.profile_id=profile.id AND check_row.status='passed' AND check_row.evidence_ref IS NOT NULL AND trim(check_row.evidence_ref) <> '')",
        [product_id], |row| row.get(0),
    )?;
    let verify_complete: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM cp_factory_stage_runs run JOIN cp_factory_cycles cycle ON cycle.id=run.cycle_id WHERE cycle.product_id=?1 AND run.stage='verify' AND run.status='completed')", [product_id], |row| row.get(0))?;
    let pending_impact_assessments: u32 = tx.query_row(
        "SELECT COUNT(*) FROM cp_factory_impact_assessments impact
         JOIN cp_factory_change_requests change ON change.id=impact.change_request_id
         WHERE change.product_id=?1 AND impact.status='ready'",
        [product_id],
        |row| row.get(0),
    )?;
    let stale_evidence_count: u32 = tx.query_row(
        "SELECT COUNT(*) FROM cp_factory_evidence_links WHERE product_id=?1 AND staleness_state='stale'",
        [product_id],
        |row| row.get(0),
    )?;
    Ok(Some(FactoryReleaseReadiness {
        required_quality_blockers: blockers,
        completed_verify_stage: verify_complete,
        pending_impact_assessments,
        stale_evidence_count,
        ready: blockers == 0
            && verify_complete
            && pending_impact_assessments == 0
            && stale_evidence_count == 0,
    }))
}
