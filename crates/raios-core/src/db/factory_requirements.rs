//! Repository boundary for requirements, revisions, links, and decisions.

use rusqlite::{params, OptionalExtension, Result, Transaction};
use uuid::Uuid;

pub const FACTORY_REQUIREMENTS_TABLE: &str = "cp_factory_requirements";
pub const FACTORY_REQUIREMENT_REVISIONS_TABLE: &str = "cp_factory_requirement_revisions";
pub const FACTORY_REQUIREMENT_LINKS_TABLE: &str = "cp_factory_requirement_links";
pub const FACTORY_DECISIONS_TABLE: &str = "cp_factory_decisions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryRequirementDraftCreated {
    pub id: String,
    pub revision_id: String,
}

pub fn create_factory_requirement_draft(
    tx: &Transaction<'_>,
    owner_subject: &str,
    product_id: &str,
    stable_key: &str,
    content: &str,
) -> Result<Option<FactoryRequirementDraftCreated>> {
    let charter_id: Option<String> = tx
        .query_row(
            "SELECT current_charter_revision_id FROM cp_factory_products
             WHERE id = ?1 AND owner_subject = ?2",
            params![product_id, owner_subject],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let Some(charter_id) = charter_id else {
        return Ok(None);
    };
    let id = Uuid::new_v4().to_string();
    let revision_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_factory_requirements (id, product_id, stable_key, status, current_revision)
         VALUES (?1, ?2, ?3, 'proposed', 1)",
        params![id, product_id, stable_key],
    )?;
    tx.execute(
        "INSERT INTO cp_factory_requirement_revisions
         (id, requirement_id, revision, content_ref, content_text, status)
         VALUES (?1, ?2, 1, ?3, ?4, 'proposed')",
        params![revision_id, id, format!("inline:{revision_id}"), content],
    )?;
    tx.execute(
        "INSERT INTO cp_factory_requirement_links (id, requirement_id, target_kind, target_id, relation_kind)
         VALUES (?1, ?2, 'charter_revision', ?3, 'derived_from')",
        params![Uuid::new_v4().to_string(), id, charter_id],
    )?;
    Ok(Some(FactoryRequirementDraftCreated { id, revision_id }))
}

pub fn apply_approved_requirement_change(
    tx: &Transaction<'_>,
    owner: &str,
    assessment_id: &str,
    requirement_id: &str,
    content: &str,
) -> Result<Option<FactoryRequirementDraftCreated>> {
    let current: Option<(i64, String)> = tx.query_row(
        "SELECT requirement.current_revision, revision.id FROM cp_factory_requirements requirement
         JOIN cp_factory_requirement_revisions revision ON revision.requirement_id=requirement.id AND revision.revision=requirement.current_revision
         JOIN cp_factory_products product ON product.id=requirement.product_id
         JOIN cp_factory_impact_assessments impact ON impact.id=?1
         JOIN cp_factory_change_requests change ON change.id=impact.change_request_id AND change.product_id=requirement.product_id
         WHERE requirement.id=?2 AND product.owner_subject=?3 AND impact.status='accepted'",
        params![assessment_id, requirement_id, owner], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    let Some((revision, previous_id)) = current else {
        return Ok(None);
    };
    let next = revision + 1;
    let revision_id = Uuid::new_v4().to_string();
    tx.execute("INSERT INTO cp_factory_requirement_revisions (id,requirement_id,revision,content_ref,content_text,status) VALUES (?1,?2,?3,?4,?5,'proposed')", params![revision_id, requirement_id, next, format!("inline:{revision_id}"), content])?;
    tx.execute("UPDATE cp_factory_requirements SET current_revision=?1, updated_at=datetime('now','utc') WHERE id=?2", params![next, requirement_id])?;
    tx.execute("INSERT INTO cp_factory_requirement_links (id,requirement_id,target_kind,target_id,relation_kind) VALUES (?1,?2,'requirement_revision',?3,'supersedes')", params![Uuid::new_v4().to_string(), requirement_id, previous_id])?;
    tx.execute(
        "UPDATE cp_factory_impact_targets SET staleness_state='current'
         WHERE assessment_id=?1 AND target_kind='requirement' AND target_id=?2",
        params![assessment_id, requirement_id],
    )?;
    tx.execute(
        "UPDATE cp_factory_evidence_links SET staleness_state='stale'
         WHERE id IN (
           SELECT dependency.evidence_id FROM cp_factory_evidence_dependencies dependency
           WHERE dependency.target_kind='requirement' AND dependency.target_id=?1
         )",
        [requirement_id],
    )?;
    Ok(Some(FactoryRequirementDraftCreated {
        id: requirement_id.into(),
        revision_id,
    }))
}
