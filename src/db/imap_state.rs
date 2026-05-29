/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderState {
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub last_seen: String,
}

pub fn get(
    conn: &Connection,
    source_id: i64,
    folder: &str,
) -> Result<Option<FolderState>, rusqlite::Error> {
    conn.query_row(
        "SELECT uidvalidity, uidnext, last_seen FROM imap_folder_state
         WHERE source_id = ?1 AND folder = ?2",
        params![source_id, folder],
        |row| {
            Ok(FolderState {
                uidvalidity: row.get(0)?,
                uidnext: row.get(1)?,
                last_seen: row.get(2)?,
            })
        },
    )
    .optional()
}

pub fn upsert(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    uidvalidity: u32,
    uidnext: u32,
    last_seen_rfc3339: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO imap_folder_state (source_id, folder, uidvalidity, uidnext, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (source_id, folder)
         DO UPDATE SET uidvalidity = excluded.uidvalidity,
                       uidnext     = excluded.uidnext,
                       last_seen   = excluded.last_seen",
        params![source_id, folder, uidvalidity, uidnext, last_seen_rfc3339],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, source_id: i64, folder: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM imap_folder_state WHERE source_id = ?1 AND folder = ?2",
        params![source_id, folder],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init;
    use crate::db::sources::{SourceKey, upsert_source};

    fn setup() -> (Connection, i64) {
        let c = Connection::open_in_memory().unwrap();
        init::apply_schema(&c).unwrap();
        let sid = upsert_source(
            &c,
            &SourceKey {
                kind: "imap".to_owned(),
                session_url: "imaps://host:993".to_owned(),
                account_id: "alice".to_owned(),
            },
            None,
            "alice",
        )
        .unwrap();
        (c, sid)
    }

    #[test]
    fn upsert_then_get_returns_latest() {
        let (c, sid) = setup();
        assert!(get(&c, sid, "INBOX").unwrap().is_none());
        upsert(&c, sid, "INBOX", 100, 50, "2026-05-24T00:00:00Z").unwrap();
        let st = get(&c, sid, "INBOX").unwrap().unwrap();
        assert_eq!(st.uidvalidity, 100);
        assert_eq!(st.uidnext, 50);
        upsert(&c, sid, "INBOX", 100, 75, "2026-05-24T01:00:00Z").unwrap();
        let st = get(&c, sid, "INBOX").unwrap().unwrap();
        assert_eq!(st.uidnext, 75);
        assert_eq!(st.last_seen, "2026-05-24T01:00:00Z");
    }

    #[test]
    fn delete_removes_only_named_folder() {
        let (c, sid) = setup();
        upsert(&c, sid, "INBOX", 1, 1, "2026-05-24T00:00:00Z").unwrap();
        upsert(&c, sid, "Sent", 2, 1, "2026-05-24T00:00:00Z").unwrap();
        delete(&c, sid, "INBOX").unwrap();
        assert!(get(&c, sid, "INBOX").unwrap().is_none());
        assert!(get(&c, sid, "Sent").unwrap().is_some());
    }
}
