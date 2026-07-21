//! Read-only `workspace.db` size/row budget check.
//!
//! Runs `SELECT COUNT(*)` against the hot control-plane/memory tables and
//! `PRAGMA page_count * PRAGMA page_size` for total file size, comparing
//! both against hardcoded soft caps. Never writes anything — see
//! `docs/BUDGET.md` at the repo root for the caps table and what happens
//! when a cap is exceeded.

use super::{DbBudgetReport, DbStorageConsumer, ProjectMemBudget, TableRowCount};
use rusqlite::{Connection, Result as SqlResult};

/// Soft cap on `mem_items` rows per project. Distillation/pruning should
/// keep this from growing without bound; start conservative, adjust once
/// real numbers are known (see docs/BUDGET.md).
pub const MEM_ITEMS_SOFT_CAP_PER_PROJECT: i64 = 5_000;

/// Soft cap on total `workspace.db` file size, in bytes.
pub const DB_TOTAL_SIZE_SOFT_CAP_BYTES: i64 = 500 * 1024 * 1024;

/// Tables whose row counts are reported by every budget check. Only
/// `mem_items` has a per-row-count soft cap today (see
/// `MEM_ITEMS_SOFT_CAP_PER_PROJECT`) — the rest are counted to establish a
/// baseline before a cap is set.
///
/// `table_row_count` interpolates these names directly into a query
/// string, so this list must only ever contain fixed, compile-time-known
/// identifiers — never anything derived from external/user input.
const BUDGET_TABLES: &[&str] = &[
    "mem_items",
    "cp_tasks",
    "cp_agent_runs",
    "cp_artifacts",
    "audit_log",
];

pub fn check() -> DbBudgetReport {
    match raios_core::db::open_db() {
        Ok(conn) => compute(&conn),
        Err(e) => error_report(format!("failed to open workspace.db: {e}")),
    }
}

fn compute(conn: &Connection) -> DbBudgetReport {
    let db_size_bytes = db_size_bytes(conn).unwrap_or(0);

    let table_counts: Vec<TableRowCount> = BUDGET_TABLES
        .iter()
        .map(|&table| TableRowCount {
            table: table.to_string(),
            row_count: table_row_count(conn, table).unwrap_or(0),
        })
        .collect();

    let mem_items_by_project = mem_items_by_project(conn).unwrap_or_default();
    let largest_storage_consumers = largest_storage_consumers(conn).unwrap_or_default();
    let mem_items_over_budget = mem_items_by_project.iter().any(|p| p.over_budget);

    DbBudgetReport {
        db_size_bytes,
        db_size_soft_cap_bytes: DB_TOTAL_SIZE_SOFT_CAP_BYTES,
        db_size_over_budget: db_size_bytes > DB_TOTAL_SIZE_SOFT_CAP_BYTES,
        table_counts,
        mem_items_by_project,
        mem_items_over_budget,
        largest_storage_consumers,
        error: None,
    }
}

fn error_report(message: String) -> DbBudgetReport {
    DbBudgetReport {
        db_size_bytes: 0,
        db_size_soft_cap_bytes: DB_TOTAL_SIZE_SOFT_CAP_BYTES,
        db_size_over_budget: false,
        table_counts: Vec::new(),
        mem_items_by_project: Vec::new(),
        mem_items_over_budget: false,
        largest_storage_consumers: Vec::new(),
        error: Some(message),
    }
}

fn largest_storage_consumers(conn: &Connection) -> SqlResult<Vec<DbStorageConsumer>> {
    let mut stmt = conn.prepare(
        "SELECT name, SUM(pgsize) AS bytes FROM dbstat GROUP BY name ORDER BY bytes DESC LIMIT 5",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(DbStorageConsumer {
            name: row.get(0)?,
            bytes: row.get(1)?,
        })
    })?;
    rows.collect()
}

fn db_size_bytes(conn: &Connection) -> SqlResult<i64> {
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    Ok(page_count * page_size)
}

/// `table` must come only from `BUDGET_TABLES` — rusqlite has no bind
/// parameter for identifiers (table/column names), only for values, so this
/// intentionally uses string interpolation over a fixed, non-user-supplied
/// list rather than a parameterized query.
fn table_row_count(conn: &Connection, table: &str) -> SqlResult<i64> {
    let query = match table {
        "mem_items" => "SELECT COUNT(*) FROM mem_items",
        "cp_tasks" => "SELECT COUNT(*) FROM cp_tasks",
        "cp_agent_runs" => "SELECT COUNT(*) FROM cp_agent_runs",
        "cp_artifacts" => "SELECT COUNT(*) FROM cp_artifacts",
        "audit_log" => "SELECT COUNT(*) FROM audit_log",
        _ => return Err(rusqlite::Error::QueryReturnedNoRows),
    };
    conn.query_row(query, [], |r| r.get(0))
}

fn mem_items_by_project(conn: &Connection) -> SqlResult<Vec<ProjectMemBudget>> {
    let mut stmt = conn.prepare(
        "SELECT project_key, COUNT(*) AS n FROM mem_items GROUP BY project_key ORDER BY n DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        let project_key: String = r.get(0)?;
        let row_count: i64 = r.get(1)?;
        Ok(ProjectMemBudget {
            project_key,
            row_count,
            soft_cap: MEM_ITEMS_SOFT_CAP_PER_PROJECT,
            over_budget: row_count > MEM_ITEMS_SOFT_CAP_PER_PROJECT,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        conn
    }

    fn insert_mem_item(conn: &Connection, project_key: &str, slug: &str) {
        conn.execute(
            "INSERT INTO mem_items (id, project_key, item_type, slug, title)
             VALUES (?1, ?2, 'reference', ?3, ?3)",
            rusqlite::params![format!("{project_key}-{slug}"), project_key, slug],
        )
        .unwrap();
    }

    #[test]
    fn empty_db_is_within_budget() {
        let conn = setup();
        let report = compute(&conn);

        assert!(report.error.is_none());
        assert!(!report.db_size_over_budget);
        assert!(!report.mem_items_over_budget);
        assert!(report.mem_items_by_project.is_empty());
        assert_eq!(report.table_counts.len(), BUDGET_TABLES.len());
        for t in &report.table_counts {
            assert_eq!(t.row_count, 0, "table {} should start empty", t.table);
        }
        assert!(report.db_size_bytes > 0, "sqlite header alone is nonzero");
    }

    #[test]
    fn mem_items_over_cap_is_flagged_per_project_not_globally() {
        let conn = setup();
        for i in 0..(MEM_ITEMS_SOFT_CAP_PER_PROJECT + 5) {
            insert_mem_item(&conn, "hot_project", &format!("slug-{i}"));
        }
        insert_mem_item(&conn, "quiet_project", "slug-only");

        let report = compute(&conn);

        assert!(report.mem_items_over_budget);

        let hot = report
            .mem_items_by_project
            .iter()
            .find(|p| p.project_key == "hot_project")
            .expect("hot_project present");
        assert_eq!(hot.row_count, MEM_ITEMS_SOFT_CAP_PER_PROJECT + 5);
        assert!(hot.over_budget);

        let quiet = report
            .mem_items_by_project
            .iter()
            .find(|p| p.project_key == "quiet_project")
            .expect("quiet_project present");
        assert_eq!(quiet.row_count, 1);
        assert!(!quiet.over_budget);

        let mem_table_count = report
            .table_counts
            .iter()
            .find(|t| t.table == "mem_items")
            .unwrap();
        assert_eq!(
            mem_table_count.row_count,
            MEM_ITEMS_SOFT_CAP_PER_PROJECT + 6
        );
    }

    #[test]
    fn counts_every_budget_table_without_error() {
        let conn = setup();
        for &table in BUDGET_TABLES {
            table_row_count(&conn, table).unwrap_or_else(|e| {
                panic!("counting {table} should not error: {e}");
            });
        }
    }

    // `check()` itself (the `open_db()` -> real, fixed
    // `~/.config/raios/workspace.db` path) is deliberately not exercised
    // here — `open_db()` has no path-injection seam, and this codebase's
    // existing convention (see daemon/handlers.rs test_client_handle's
    // doc comment) is to never let tests touch that shared file. `compute()`
    // above covers all of `check()`'s logic against an isolated in-memory DB.
}
