/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

pub const MAILBOX: &str = "mailbox";
pub const EMAIL: &str = "email";
pub const ADDRESS_BOOK: &str = "addressbook";
pub const CONTACT_CARD: &str = "contactcard";
pub const CALENDAR: &str = "calendar";
pub const CALENDAR_EVENT: &str = "calendarevent";

pub fn insert(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    source_obj_id: &str,
    local_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO sync_id_takeout
         (source_id, type_name, source_obj_id, local_id)
         VALUES (?1, ?2, ?3, ?4)",
        params![source_id, type_name, source_obj_id, local_id],
    )?;
    Ok(())
}

pub fn local_for(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    source_obj_id: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT local_id FROM sync_id_takeout
         WHERE source_id = ?1 AND type_name = ?2 AND source_obj_id = ?3",
        params![source_id, type_name, source_obj_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn delete(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    source_obj_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_takeout
         WHERE source_id = ?1 AND type_name = ?2 AND source_obj_id = ?3",
        params![source_id, type_name, source_obj_id],
    )?;
    Ok(())
}

pub fn all_for_type(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
) -> Result<HashMap<String, i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT source_obj_id, local_id FROM sync_id_takeout
         WHERE source_id = ?1 AND type_name = ?2",
    )?;
    let rows = stmt.query_map(params![source_id, type_name], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = HashMap::new();
    for r in rows {
        let (k, v) = r?;
        map.insert(k, v);
    }
    Ok(map)
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
                kind: "takeout".to_owned(),
                session_url: "file:///tmp/Takeout".to_owned(),
                account_id: "/tmp/Takeout".to_owned(),
            },
            Some("Takeout"),
            "",
        )
        .unwrap();
        (c, sid)
    }

    #[test]
    fn insert_and_lookup_roundtrip() {
        let (c, sid) = setup();
        insert(&c, sid, MAILBOX, "Inbox", 7).unwrap();
        insert(&c, sid, MAILBOX, "Label/Sub", 8).unwrap();
        assert_eq!(local_for(&c, sid, MAILBOX, "Inbox").unwrap(), Some(7));
        assert_eq!(local_for(&c, sid, MAILBOX, "Label/Sub").unwrap(), Some(8));
        assert_eq!(local_for(&c, sid, MAILBOX, "Missing").unwrap(), None);
    }

    #[test]
    fn types_are_isolated_namespaces() {
        let (c, sid) = setup();
        insert(&c, sid, MAILBOX, "Inbox", 1).unwrap();
        insert(&c, sid, EMAIL, "ab12", 2).unwrap();
        insert(&c, sid, ADDRESS_BOOK, "Imported", 3).unwrap();
        insert(&c, sid, CONTACT_CARD, "uid-1", 4).unwrap();
        insert(&c, sid, CALENDAR, "Imported", 5).unwrap();
        insert(&c, sid, CALENDAR_EVENT, "uid-e1", 6).unwrap();
        assert_eq!(all_for_type(&c, sid, MAILBOX).unwrap().len(), 1);
        assert_eq!(all_for_type(&c, sid, EMAIL).unwrap().len(), 1);
        assert_eq!(all_for_type(&c, sid, ADDRESS_BOOK).unwrap().len(), 1);
        assert_eq!(all_for_type(&c, sid, CONTACT_CARD).unwrap().len(), 1);
        assert_eq!(all_for_type(&c, sid, CALENDAR).unwrap().len(), 1);
        assert_eq!(all_for_type(&c, sid, CALENDAR_EVENT).unwrap().len(), 1);
    }

    #[test]
    fn delete_removes_only_target_row() {
        let (c, sid) = setup();
        insert(&c, sid, EMAIL, "hash-a", 1).unwrap();
        insert(&c, sid, EMAIL, "hash-b", 2).unwrap();
        delete(&c, sid, EMAIL, "hash-a").unwrap();
        assert_eq!(local_for(&c, sid, EMAIL, "hash-a").unwrap(), None);
        assert_eq!(local_for(&c, sid, EMAIL, "hash-b").unwrap(), Some(2));
    }

    #[test]
    fn pk_rejects_duplicate_source_obj_id() {
        let (c, sid) = setup();
        insert(&c, sid, EMAIL, "h", 1).unwrap();
        let err = insert(&c, sid, EMAIL, "h", 2).expect_err("PK conflict");
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    #[test]
    fn unique_constraint_rejects_duplicate_local_id_within_type() {
        let (c, sid) = setup();
        insert(&c, sid, EMAIL, "h1", 42).unwrap();
        let err = insert(&c, sid, EMAIL, "h2", 42).expect_err("UNIQUE conflict");
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    #[test]
    fn same_local_id_allowed_across_different_types() {
        let (c, sid) = setup();
        insert(&c, sid, MAILBOX, "Inbox", 42).unwrap();
        insert(&c, sid, EMAIL, "h1", 42).unwrap();
    }

    #[test]
    fn all_for_type_returns_full_map() {
        let (c, sid) = setup();
        insert(&c, sid, EMAIL, "h1", 1).unwrap();
        insert(&c, sid, EMAIL, "h2", 2).unwrap();
        insert(&c, sid, EMAIL, "h3", 3).unwrap();
        let all = all_for_type(&c, sid, EMAIL).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all.get("h1"), Some(&1));
        assert_eq!(all.get("h3"), Some(&3));
    }
}
