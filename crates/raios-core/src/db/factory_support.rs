//! Repository boundary for support items.

use rusqlite::{params, OptionalExtension, Result, Transaction};
use uuid::Uuid;

pub const FACTORY_SUPPORT_ITEMS_TABLE: &str = "cp_factory_support_items";
pub const FACTORY_SUPPORT_LINKS_TABLE: &str = "cp_factory_support_links";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactorySupportOverview {
    pub open_count: u32,
    pub resolved_count: u32,
    pub linked_change_count: u32,
    pub oldest_open_created_at: Option<String>,
}

pub fn factory_support_overview(
    tx: &Transaction<'_>,
    owner: &str,
    product_id: &str,
) -> Result<Option<FactorySupportOverview>> {
    let owned: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_factory_products WHERE id=?1 AND owner_subject=?2)",
        params![product_id, owner],
        |row| row.get(0),
    )?;
    if !owned {
        return Ok(None);
    }
    let open_count = tx.query_row("SELECT COUNT(*) FROM cp_factory_support_items WHERE product_id=?1 AND status IN ('new','triaged','in_progress')", [product_id], |row| row.get(0))?;
    let resolved_count = tx.query_row(
        "SELECT COUNT(*) FROM cp_factory_support_items WHERE product_id=?1 AND status='resolved'",
        [product_id],
        |row| row.get(0),
    )?;
    let linked_change_count = tx.query_row("SELECT COUNT(*) FROM cp_factory_support_links link JOIN cp_factory_support_items item ON item.id=link.support_item_id WHERE item.product_id=?1 AND link.target_kind='change_request'", [product_id], |row| row.get(0))?;
    let oldest_open_created_at = tx.query_row("SELECT created_at FROM cp_factory_support_items WHERE product_id=?1 AND status IN ('new','triaged','in_progress') ORDER BY created_at ASC LIMIT 1", [product_id], |row| row.get(0)).optional()?;
    Ok(Some(FactorySupportOverview {
        open_count,
        resolved_count,
        linked_change_count,
        oldest_open_created_at,
    }))
}

pub fn create_factory_support_item(
    tx: &Transaction<'_>,
    owner: &str,
    product_id: &str,
    source_kind: &str,
    summary: &str,
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
    tx.execute("INSERT INTO cp_factory_support_items (id, product_id, source_kind, status, summary_ref) VALUES (?1,?2,?3,'new',?4)", params![id, product_id, source_kind, summary])?;
    Ok(Some(id))
}

pub fn triage_factory_support_item(
    tx: &Transaction<'_>,
    owner: &str,
    item_id: &str,
) -> Result<bool> {
    Ok(tx.execute("UPDATE cp_factory_support_items SET status='triaged' WHERE id=?1 AND status='new' AND product_id IN (SELECT id FROM cp_factory_products WHERE owner_subject=?2)", params![item_id, owner])? == 1)
}

pub fn resolve_factory_support_item(
    tx: &Transaction<'_>,
    owner: &str,
    item_id: &str,
    resolution_ref: &str,
) -> Result<bool> {
    Ok(tx.execute(
        "UPDATE cp_factory_support_items SET status='resolved', resolution_ref=?1, resolved_at=datetime('now','utc')
         WHERE id=?2 AND status IN ('new','triaged','in_progress')
           AND product_id IN (SELECT id FROM cp_factory_products WHERE owner_subject=?3)",
        params![resolution_ref, item_id, owner],
    )? == 1)
}

pub fn link_support_to_change_request(
    tx: &Transaction<'_>,
    owner: &str,
    item_id: &str,
    change_id: &str,
) -> Result<bool> {
    let same_product: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM cp_factory_support_items item JOIN cp_factory_change_requests change ON change.product_id=item.product_id JOIN cp_factory_products product ON product.id=item.product_id WHERE item.id=?1 AND change.id=?2 AND product.owner_subject=?3)", params![item_id, change_id, owner], |row| row.get(0))?;
    if !same_product {
        return Ok(false);
    }
    tx.execute("INSERT OR IGNORE INTO cp_factory_support_links (id, support_item_id, target_kind, target_id, relation_kind) VALUES (?1,?2,'change_request',?3,'raised_by')", params![Uuid::new_v4().to_string(), item_id, change_id])?;
    Ok(true)
}
