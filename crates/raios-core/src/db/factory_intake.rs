//! Repository boundary for Product Factory discovery and charter intake.

use rusqlite::{params, Connection, OptionalExtension, Result, Transaction};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::product_factory::prompts_for_mode;

pub const FACTORY_INTAKE_SESSIONS_TABLE: &str = "cp_factory_intake_sessions";
pub const FACTORY_INTAKE_ITEMS_TABLE: &str = "cp_factory_intake_items";
pub const FACTORY_CHARTER_REVISIONS_TABLE: &str = "cp_factory_charter_revisions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryIntakeSessionCreated {
    pub id: String,
    pub product_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryCharterDraftCreated {
    pub id: String,
    pub product_id: String,
    pub revision: u32,
}

pub fn start_factory_intake(
    tx: &Transaction<'_>,
    owner_subject: &str,
    product_id: &str,
) -> Result<Option<FactoryIntakeSessionCreated>> {
    let owns_product: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_factory_products WHERE id = ?1 AND owner_subject = ?2)",
        params![product_id, owner_subject],
        |row| row.get(0),
    )?;
    if !owns_product {
        return Ok(None);
    }

    let existing = tx
        .query_row(
            "SELECT id FROM cp_factory_intake_sessions
             WHERE product_id = ?1 AND status = 'open' ORDER BY created_at DESC, id DESC LIMIT 1",
            [product_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(id) = existing {
        ensure_discovery_intake_prompts(tx, &id, product_id)?;
        return Ok(Some(FactoryIntakeSessionCreated {
            id,
            product_id: product_id.into(),
        }));
    }

    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_factory_intake_sessions (id, product_id, status, started_by)
         VALUES (?1, ?2, 'open', ?3)",
        params![id, product_id, owner_subject],
    )?;
    ensure_discovery_intake_prompts(tx, &id, product_id)?;
    Ok(Some(FactoryIntakeSessionCreated {
        id,
        product_id: product_id.into(),
    }))
}

fn ensure_discovery_intake_prompts(
    tx: &Transaction<'_>,
    session_id: &str,
    product_id: &str,
) -> Result<()> {
    let mode: String = tx.query_row(
        "SELECT factory_mode FROM cp_factory_products WHERE id = ?1",
        [product_id],
        |row| row.get(0),
    )?;
    for prompt in prompts_for_mode(&mode) {
        tx.execute(
            "INSERT INTO cp_factory_intake_items
             (id, session_id, item_kind, question_key, prompt_ref, status)
             SELECT ?1, ?2, 'question', ?3, ?4, 'open'
             WHERE NOT EXISTS (
                 SELECT 1 FROM cp_factory_intake_items WHERE session_id = ?2 AND question_key = ?3
             )",
            params![
                Uuid::new_v4().to_string(),
                session_id,
                prompt.key,
                format!("builtin:discovery/v1/{}", prompt.key),
            ],
        )?;
    }
    Ok(())
}

/// Returns the required discovery prompt keys that still lack a non-empty
/// answer in the product's current open intake session.
pub fn missing_required_intake_prompt_keys(
    tx: &Transaction<'_>,
    product_id: &str,
) -> Result<Vec<&'static str>> {
    let session_id = tx
        .query_row(
            "SELECT id FROM cp_factory_intake_sessions
             WHERE product_id = ?1 AND status = 'open'
             ORDER BY created_at DESC, id DESC LIMIT 1",
            [product_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let mode: String = tx.query_row(
        "SELECT factory_mode FROM cp_factory_products WHERE id = ?1",
        [product_id],
        |row| row.get(0),
    )?;
    let prompts = prompts_for_mode(&mode);
    let Some(session_id) = session_id else {
        return Ok(prompts
            .iter()
            .filter(|prompt| prompt.required)
            .map(|prompt| prompt.key)
            .collect());
    };

    let mut missing = Vec::new();
    for prompt in prompts.iter().filter(|prompt| prompt.required) {
        let answered: bool = tx.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM cp_factory_intake_items
                 WHERE session_id = ?1 AND question_key = ?2
                   AND status = 'answered' AND trim(response_text) <> ''
             )",
            params![session_id, prompt.key],
            |row| row.get(0),
        )?;
        if !answered {
            missing.push(prompt.key);
        }
    }
    Ok(missing)
}

pub fn load_required_intake_answers(
    tx: &Transaction<'_>,
    product_id: &str,
) -> Result<BTreeMap<String, String>> {
    let session_id: String = tx.query_row(
        "SELECT id FROM cp_factory_intake_sessions
         WHERE product_id = ?1 AND status = 'open'
         ORDER BY created_at DESC, id DESC LIMIT 1",
        [product_id],
        |row| row.get(0),
    )?;
    let mut answers = BTreeMap::new();
    let mode: String = tx.query_row(
        "SELECT factory_mode FROM cp_factory_products WHERE id = ?1",
        [product_id],
        |row| row.get(0),
    )?;
    for prompt in prompts_for_mode(&mode)
        .iter()
        .filter(|prompt| prompt.required)
    {
        let response: String = tx.query_row(
            "SELECT response_text FROM cp_factory_intake_items
             WHERE session_id = ?1 AND question_key = ?2 AND status = 'answered'",
            params![session_id, prompt.key],
            |row| row.get(0),
        )?;
        answers.insert(prompt.key.into(), response);
    }
    Ok(answers)
}

pub fn record_factory_intake_answer(
    tx: &Transaction<'_>,
    owner_subject: &str,
    session_id: &str,
    question_key: &str,
    response: &str,
) -> Result<bool> {
    let owns_session: bool = tx.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM cp_factory_intake_sessions session
             JOIN cp_factory_products product ON product.id = session.product_id
             WHERE session.id = ?1 AND product.owner_subject = ?2 AND session.status = 'open'
         )",
        params![session_id, owner_subject],
        |row| row.get(0),
    )?;
    if !owns_session {
        return Ok(false);
    }

    let existing = tx
        .query_row(
            "SELECT id FROM cp_factory_intake_items WHERE session_id = ?1 AND question_key = ?2",
            params![session_id, question_key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(id) = existing {
        tx.execute(
            "UPDATE cp_factory_intake_items
             SET response_text = ?1, response_ref = NULL, status = 'answered', responded_at = datetime('now','utc')
             WHERE id = ?2",
            params![response, id],
        )?;
    } else {
        tx.execute(
            "INSERT INTO cp_factory_intake_items
             (id, session_id, item_kind, question_key, response_text, status, responded_at)
             VALUES (?1, ?2, 'question', ?3, ?4, 'answered', datetime('now','utc'))",
            params![
                Uuid::new_v4().to_string(),
                session_id,
                question_key,
                response
            ],
        )?;
    }
    Ok(true)
}

pub fn create_factory_charter_draft(
    tx: &Transaction<'_>,
    owner_subject: &str,
    product_id: &str,
    content: &str,
) -> Result<Option<FactoryCharterDraftCreated>> {
    let owns_product: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_factory_products WHERE id = ?1 AND owner_subject = ?2)",
        params![product_id, owner_subject],
        |row| row.get(0),
    )?;
    if !owns_product {
        return Ok(None);
    }
    let latest_revision: i64 = tx.query_row(
        "SELECT COALESCE(MAX(revision), 0) FROM cp_factory_charter_revisions WHERE product_id = ?1",
        [product_id],
        |row| row.get(0),
    )?;
    let revision = u32::try_from(latest_revision + 1).unwrap_or(u32::MAX);
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_factory_charter_revisions
         (id, product_id, revision, status, content_ref, content_text, created_by)
         VALUES (?1, ?2, ?3, 'proposed', ?4, ?5, ?6)",
        params![
            id,
            product_id,
            revision,
            format!("inline:{id}"),
            content,
            owner_subject
        ],
    )?;
    tx.execute(
        "UPDATE cp_factory_products SET current_charter_revision_id = ?1, updated_at = datetime('now','utc') WHERE id = ?2",
        params![id, product_id],
    )?;
    Ok(Some(FactoryCharterDraftCreated {
        id,
        product_id: product_id.into(),
        revision,
    }))
}

pub fn factory_intake_session_owned_by(
    conn: &Connection,
    session_id: &str,
    owner_subject: &str,
) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM cp_factory_intake_sessions session
             JOIN cp_factory_products product ON product.id = session.product_id
             WHERE session.id = ?1 AND product.owner_subject = ?2
         )",
        params![session_id, owner_subject],
        |row| row.get(0),
    )
}
