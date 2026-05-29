/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

pub const MAILBOX: &str = "mailbox";
pub const CALENDAR: &str = "calendar";
pub const ADDRESS_BOOK: &str = "addressbook";
pub const EMAIL: &str = "email";
pub const CALENDAR_EVENT: &str = "calendarevent";
pub const CONTACT_CARD: &str = "contactcard";

#[derive(Debug, Clone)]
pub struct ItemRow {
    pub item_id: String,
    pub change_key: String,
    pub local_id: i64,
}

#[derive(Debug, Clone)]
pub struct FolderRow {
    pub item_id: String,
    pub folder_id: String,
    pub change_key: String,
    pub local_id: i64,
}

pub fn insert(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    folder_id: &str,
    item_id: &str,
    change_key: &str,
    local_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO sync_id_exchange_ews
         (source_id, type_name, folder_id, item_id, change_key, sync_state, local_id)
         VALUES (?1, ?2, ?3, ?4, ?5, '', ?6)",
        params![
            source_id, type_name, folder_id, item_id, change_key, local_id
        ],
    )?;
    Ok(())
}

pub fn update_change_key(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    item_id: &str,
    change_key: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE sync_id_exchange_ews
         SET change_key = ?4
         WHERE source_id = ?1 AND type_name = ?2 AND item_id = ?3",
        params![source_id, type_name, item_id, change_key],
    )?;
    Ok(())
}

pub fn delete_item(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    item_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_exchange_ews
         WHERE source_id = ?1 AND type_name = ?2 AND item_id = ?3",
        params![source_id, type_name, item_id],
    )?;
    Ok(())
}

pub fn local_for_item(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    item_id: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT local_id FROM sync_id_exchange_ews
         WHERE source_id = ?1 AND type_name = ?2 AND item_id = ?3",
        params![source_id, type_name, item_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn items_in_folder(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    folder_id: &str,
) -> Result<Vec<ItemRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT item_id, change_key, local_id FROM sync_id_exchange_ews
         WHERE source_id = ?1 AND type_name = ?2 AND folder_id = ?3",
    )?;
    let rows = stmt.query_map(params![source_id, type_name, folder_id], |row| {
        Ok(ItemRow {
            item_id: row.get(0)?,
            change_key: row.get(1)?,
            local_id: row.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn folders_of_type(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
) -> Result<HashMap<String, FolderRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT item_id, folder_id, change_key, local_id FROM sync_id_exchange_ews
         WHERE source_id = ?1 AND type_name = ?2",
    )?;
    let rows = stmt.query_map(params![source_id, type_name], |row| {
        Ok(FolderRow {
            item_id: row.get(0)?,
            folder_id: row.get(1)?,
            change_key: row.get(2)?,
            local_id: row.get(3)?,
        })
    })?;
    let mut map = HashMap::new();
    for r in rows {
        let f = r?;
        map.insert(f.item_id.clone(), f);
    }
    Ok(map)
}

pub fn get_sync_state(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    item_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT sync_state FROM sync_id_exchange_ews
         WHERE source_id = ?1 AND type_name = ?2 AND item_id = ?3",
        params![source_id, type_name, item_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

pub fn set_sync_state(
    conn: &Connection,
    source_id: i64,
    type_name: &str,
    item_id: &str,
    sync_state: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE sync_id_exchange_ews
         SET sync_state = ?4
         WHERE source_id = ?1 AND type_name = ?2 AND item_id = ?3",
        params![source_id, type_name, item_id, sync_state],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init;
    use crate::db::sources::{SourceKey, upsert_source};

    fn setup() -> (rusqlite::Connection, i64) {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init::apply_schema(&conn).unwrap();
        let key = SourceKey {
            kind: "exchange_ews".to_owned(),
            session_url: "https://x/EWS/Exchange.asmx".to_owned(),
            account_id: "u@d".to_owned(),
        };
        let sid = upsert_source(&conn, &key, Some("u"), "u@d").unwrap();
        (conn, sid)
    }

    #[test]
    fn insert_and_lookup_round_trip() {
        let (conn, sid) = setup();
        insert(&conn, sid, EMAIL, "FOLDER1", "ITEM1", "CK1", 42).unwrap();
        let id = local_for_item(&conn, sid, EMAIL, "ITEM1").unwrap().unwrap();
        assert_eq!(id, 42);
        let rows = items_in_folder(&conn, sid, EMAIL, "FOLDER1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].change_key, "CK1");
    }

    #[test]
    fn update_change_key_preserves_local_id() {
        let (conn, sid) = setup();
        insert(&conn, sid, EMAIL, "F1", "I1", "A", 7).unwrap();
        update_change_key(&conn, sid, EMAIL, "I1", "B").unwrap();
        let rows = items_in_folder(&conn, sid, EMAIL, "F1").unwrap();
        assert_eq!(rows[0].change_key, "B");
        assert_eq!(rows[0].local_id, 7);
    }

    #[test]
    fn folders_of_type_lists_by_item_id() {
        let (conn, sid) = setup();
        insert(&conn, sid, MAILBOX, "", "F1", "CK1", 1).unwrap();
        insert(&conn, sid, MAILBOX, "F1", "F2", "CK2", 2).unwrap();
        let map = folders_of_type(&conn, sid, MAILBOX).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("F1").map(|r| r.local_id), Some(1));
        assert_eq!(map.get("F2").map(|r| r.local_id), Some(2));
    }

    #[test]
    fn sync_state_round_trips() {
        let (conn, sid) = setup();
        insert(&conn, sid, MAILBOX, "", "F1", "", 1).unwrap();
        assert_eq!(
            get_sync_state(&conn, sid, MAILBOX, "F1")
                .unwrap()
                .as_deref(),
            Some("")
        );
        set_sync_state(&conn, sid, MAILBOX, "F1", "opaque-state").unwrap();
        assert_eq!(
            get_sync_state(&conn, sid, MAILBOX, "F1")
                .unwrap()
                .as_deref(),
            Some("opaque-state")
        );
    }

    #[test]
    fn delete_item_removes_only_one() {
        let (conn, sid) = setup();
        insert(&conn, sid, EMAIL, "F1", "I1", "A", 1).unwrap();
        insert(&conn, sid, EMAIL, "F1", "I2", "A", 2).unwrap();
        delete_item(&conn, sid, EMAIL, "I1").unwrap();
        let rows = items_in_folder(&conn, sid, EMAIL, "F1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_id, "I2");
    }

    #[test]
    fn unique_local_id_per_type_is_enforced() {
        let (conn, sid) = setup();
        insert(&conn, sid, EMAIL, "F1", "I1", "A", 1).unwrap();
        let err = insert(&conn, sid, EMAIL, "F1", "I2", "A", 1).unwrap_err();
        assert!(err.to_string().contains("UNIQUE"));
    }
}
