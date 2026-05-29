/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use rusqlite::Connection;

pub const SCHEMA_SQL: &str = include_str!("schema.sql");

pub fn open(path: &std::path::Path) -> Result<Connection, OpenError> {
    let conn = Connection::open(path)?;
    apply_pragmas(&conn)?;
    apply_schema(&conn)?;
    Ok(conn)
}

pub fn apply_schema(conn: &Connection) -> Result<(), OpenError> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(SCHEMA_SQL)?;
    ensure_calendar_events_data_type(&tx)?;
    tx.commit()?;
    Ok(())
}

fn ensure_calendar_events_data_type(conn: &Connection) -> Result<(), OpenError> {
    let mut stmt = conn.prepare("PRAGMA table_info(calendar_events)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let has_column = rows.filter_map(|r| r.ok()).any(|name| name == "data_type");
    if !has_column {
        conn.execute(
            "ALTER TABLE calendar_events ADD COLUMN data_type TEXT NOT NULL DEFAULT 'Event'",
            [],
        )?;
    }
    Ok(())
}

fn apply_pragmas(conn: &Connection) -> Result<(), OpenError> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}
