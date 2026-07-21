//! Repository boundary for release records and release channels.

use rusqlite::{params, Result, Transaction};
use uuid::Uuid;

pub const FACTORY_RELEASES_TABLE: &str = "cp_factory_releases";
pub const FACTORY_RELEASE_CHANNELS_TABLE: &str = "cp_factory_release_channels";

pub fn create_factory_release_draft(
    tx: &Transaction<'_>,
    owner: &str,
    product_id: &str,
    build_ref: &str,
) -> Result<Option<String>> {
    if !super::factory_release_ready(tx, owner, product_id)?.unwrap_or(false) {
        return Ok(None);
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_factory_releases (id, product_id, status, build_ref) VALUES (?1,?2,'draft',?3)",
        params![id, product_id, build_ref],
    )?;
    Ok(Some(id))
}

pub fn approve_factory_closed_testing_release(
    tx: &Transaction<'_>,
    owner: &str,
    release_id: &str,
) -> Result<bool> {
    let owned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM cp_factory_releases release JOIN cp_factory_products product ON product.id=release.product_id WHERE release.id=?1 AND release.status='draft' AND product.owner_subject=?2)", params![release_id, owner], |row| row.get(0))?;
    if !owned {
        return Ok(false);
    }
    tx.execute(
        "UPDATE cp_factory_releases SET status='approved' WHERE id=?1",
        [release_id],
    )?;
    tx.execute("INSERT INTO cp_factory_release_channels (id, release_id, channel_kind, status) VALUES (?1,?2,'closed_testing','approved')", params![Uuid::new_v4().to_string(), release_id])?;
    Ok(true)
}
