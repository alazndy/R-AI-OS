pub use super::*;
pub use rusqlite::{params, Connection};

mod agent_stats;
mod control_plane;
mod db_path;
mod factory;
mod handoff;
mod integration;
mod mem;
mod schema;
mod tasks;
mod tool_traces;

pub fn in_memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    migrate_existing(&conn).unwrap();
    conn
}
